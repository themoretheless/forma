import {spawn} from 'node:child_process';
import {mkdir, mkdtemp, writeFile} from 'node:fs/promises';
import path from 'node:path';
import {fileURLToPath} from 'node:url';

const projectRoot = fileURLToPath(new URL('./', import.meta.url));
const maxSourceLength = 100_000;
// Linked templates expand every control and its visual parts.
const maxTemplateLength = 2_000_000;

export function createVectorRunner(send, dependencies = {}) {
  const {
    spawn: spawnProcess = spawn,
    mkdir: makeDirectory = mkdir,
    mkdtemp: makeTemporaryDirectory = mkdtemp,
    writeFile: writeSnapshot = writeFile,
    kill: signalProcess = process.kill.bind(process),
    platform = process.platform,
    root = projectRoot,
  } = dependencies;
  const grouped = platform !== 'win32';
  let active = null;

  function finish(job, code, signal) {
    if (job.finished) return;
    job.finished = true;
    clearTimeout(job.killTimer);
    if (active !== job) return;
    active = null;
    send({kind: 'finished', code, signal, text: `Векторное окно закрыто: ${signal ?? code ?? 'ошибка запуска'}`});
  }

  function kill(job, signal) {
    if (!job.child || job.finished) return;
    try {
      if (grouped && job.child.pid) {
        // Cargo and the native window share this isolated process group.
        signalProcess(-job.child.pid, signal);
      } else {
        job.child.kill(signal);
      }
    } catch (error) {
      if (error.code !== 'ESRCH') send({kind: 'error', text: `Не удалось остановить векторное окно: ${error.message}`});
    }
  }

  return {
    async run({source, template} = {}) {
      if (active) {
        send({kind: 'error', text: 'Векторное окно уже запускается или запущено. Сначала остановите его.'});
        return;
      }
      if (typeof source !== 'string' || !source.trim() || source.length > maxSourceLength) {
        send({kind: 'error', text: `Нужна непустая разметка .ui размером не больше ${maxSourceLength} символов`});
        return;
      }
      if (template !== undefined && (typeof template !== 'string' || !template.trim())) {
        send({kind: 'error', text: 'Нужен непустой шаблон компонентов Forma'}); return;
      }
      if (template !== undefined && template.length > maxTemplateLength) {
        send({kind: 'error', text: `Шаблон компонентов превышает лимит ${maxTemplateLength} символов (${template.length})`}); return;
      }

      const job = {child: null, stopped: false, finished: false, killTimer: null};
      active = job;
      try {
        const snapshotsRoot = path.join(root, '.forma');
        await makeDirectory(snapshotsRoot, {recursive: true});
        const directory = await makeTemporaryDirectory(path.join(snapshotsRoot, 'vector-'));
        const snapshot = path.join(directory, 'snapshot.ui');
        // Keep snapshots: subsequent edits or launches must not overwrite this window's source.
        await writeSnapshot(snapshot, source, 'utf8');
        const component = path.join(directory, 'Button.ui');
        if (template !== undefined) await writeSnapshot(component, template, 'utf8');
        if (job.stopped) {
          finish(job, null, 'SIGTERM');
          return;
        }

        send({kind: 'started', text: 'Сборка векторного окна из разметки Forma Studio…'});
        const child = spawnProcess('cargo', [
          'run', '--release', '--offline', '--manifest-path', path.join(root, 'vector-ui/Cargo.toml'),
          '--features', 'native', '--', snapshot, ...(template === undefined ? [] : [component]),
        ], {cwd: root, detached: grouped, stdio: ['ignore', 'pipe', 'pipe']});
        job.child = child;
        for (const channel of ['stdout', 'stderr']) {
          child[channel].on('data', data => {
            if (active === job && !job.finished) send({kind: channel, text: data.toString()});
          });
        }
        child.once('error', error => {
          if (active !== job || job.finished) return;
          send({kind: 'error', text: error.message});
          // A failed spawn has no process to wait for. Its later close event is stale.
          if (!child.pid) finish(job, null, null);
        });
        child.once('close', (code, signal) => finish(job, code, signal));
      } catch (error) {
        if (active === job && !job.finished) {
          send({kind: 'error', text: error.message});
          finish(job, null, null);
        }
      }
    },

    stop() {
      const job = active;
      if (!job || job.stopped) return;
      job.stopped = true;
      if (!job.child) return;
      kill(job, 'SIGTERM');
      // Also cover a hung build/window; keep the busy guard until its streams close.
      job.killTimer = setTimeout(() => kill(job, 'SIGKILL'), 1500);
      job.killTimer.unref();
    },
  };
}
