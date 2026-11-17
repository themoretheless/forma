import { spawn } from 'node:child_process';
import { mkdir, rename, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const projectRoot = fileURLToPath(new URL('./', import.meta.url));

export function createNativeRunner(send, dependencies = {}) {
  const {
    spawn: spawnProcess = spawn,
    mkdir: makeDirectory = mkdir,
    rename: renameFile = dependencies.writeFile ? null : rename,
    writeFile: writeSnapshot = writeFile,
    kill: signalProcess = process.kill.bind(process),
    platform = process.platform,
    root = projectRoot,
  } = dependencies;
  const grouped = platform !== 'win32';
  let child = null,
    busy = false,
    active = null;
  function kill(job, signal) {
    if (!job.child || job.finished) return;
    try {
      if (grouped && job.child.pid) {
        signalProcess(-job.child.pid, signal);
      } else {
        job.child.kill(signal);
      }
    } catch (error) {
      if (error.code !== 'ESRCH')
        send({ kind: 'error', text: `Не удалось остановить окно: ${error.message}` });
    }
  }
  return {
    async run(request) {
      if (busy) {
        send({ kind: 'error', text: 'Окно уже запущено' });
        return;
      }
      busy = true;
      const job = {
        child: null,
        stopped: false,
        finished: false,
        killTimer: null,
      };
      active = job;
      const release = () => {
        if (job.finished) return false;
        job.finished = true;
        clearTimeout(job.killTimer);
        if (active === job) {
          active = null;
          child = null;
          busy = false;
        }
        return true;
      };
      try {
        const { files, snapshot } = request ?? {};
        if (!snapshot?.html || typeof files?.['src/actions.rs'] !== 'string')
          throw Error('Нужны корректный preview и src/actions.rs');
        const formaDir = path.join(root, '.forma');
        const snapshotPath = path.join(formaDir, 'native-snapshot.json');
        const actionsPath = path.join(formaDir, 'native-actions.rs');
        await makeDirectory(formaDir, { recursive: true });
        await writeSnapshot(snapshotPath, JSON.stringify(snapshot));
        await writeSnapshot(actionsPath, files['src/actions.rs']);
        if (job.stopped) {
          if (release())
            send({
              kind: 'finished',
              code: null,
              signal: 'SIGTERM',
              text: 'Окно закрыто, код SIGTERM',
            });
          return;
        }
        send({ kind: 'started', text: 'Сборка нативного окна…' });
        const proc = spawnProcess(
          'cargo',
          [
            'run',
            '--offline',
            '--manifest-path',
            path.join(root, 'native-app/Cargo.toml'),
            '--',
            snapshotPath,
          ],
          {
            cwd: root,
            detached: grouped,
            env: { ...process.env, FORMA_ACTIONS: actionsPath },
            stdio: ['ignore', 'pipe', 'pipe'],
          },
        );
        child = proc;
        job.child = proc;
        for (const channel of ['stdout', 'stderr'])
          proc[channel].on('data', data => {
            if (!job.finished) send({ kind: channel, text: data.toString() });
          });
        proc.once('error', e => {
          if (!release()) return;
          send({ kind: 'error', text: e.message });
        });
        proc.once('close', (code, signal) => {
          if (!release()) return;
          send({ kind: 'finished', code, signal, text: 'Окно закрыто, код ' + (signal ?? code) });
        });
      } catch (e) {
        if (release()) send({ kind: 'error', text: e.message });
      }
    },
    async update({ snapshot } = {}) {
      const job = active;
      if (!busy || !job || job.stopped || job.finished || !snapshot?.html) return false;
      try {
        const formaDir = path.join(root, '.forma');
        const snapshotPath = path.join(formaDir, 'native-snapshot.json');
        await makeDirectory(formaDir, { recursive: true });
        if (renameFile) {
          const tmpPath = `${snapshotPath}.tmp`;
          await writeSnapshot(tmpPath, JSON.stringify(snapshot));
          await renameFile(tmpPath, snapshotPath);
        } else {
          await writeSnapshot(snapshotPath, JSON.stringify(snapshot));
        }
        return true;
      } catch {
        return false;
      }
    },
    stop() {
      const job = active;
      if (!job || job.stopped) return;
      job.stopped = true;
      if (!child) return;
      kill(job, 'SIGTERM');
      job.killTimer = setTimeout(() => kill(job, 'SIGKILL'), 1500);
      job.killTimer.unref();
    },
  };
}
