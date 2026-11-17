import { prepareFormProject, mapFormDiagnostic } from './form-project.js';
import { spawn } from 'node:child_process';
import { mkdtemp, writeFile, mkdir, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
const maxFileCount = 64,
  maxTotalLength = 1_000_000;
export function createRustRunner(send, dependencies = {}) {
  const {
    spawn: spawnProcess = spawn,
    mkdtemp: makeTemporaryDirectory = mkdtemp,
    writeFile: writeSourceFile = writeFile,
    mkdir: makeDirectory = mkdir,
    rm: removeDirectory = rm,
    kill: signalProcess = process.kill.bind(process),
    platform = process.platform,
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
        send({ kind: 'error', text: `Не удалось остановить Rust: ${error.message}` });
    }
  }
  return {
    async run(files, options = {}) {
      if (busy) {
        send({ kind: 'error', text: 'Rust уже запущен' });
        return;
      }
      busy = true;
      let directory = null,
        cleaned = false;
      const cleanup = () => {
        if (cleaned || !directory) return;
        cleaned = true;
        const target = directory;
        directory = null;
        Promise.resolve(removeDirectory(target, { recursive: true, force: true })).catch(() => {});
      };
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
        cleanup();
        return true;
      };
      try {
        if (!files || typeof files !== 'object' || Array.isArray(files))
          throw Error('Ожидаются файлы проекта');
        let sourceMap = {};
        if (options.generated) {
          const prepared = prepareFormProject(files, options.state, { nativeApp: options.native });
          files = prepared.files;
          sourceMap = prepared.sourceMap;
          send({ kind: 'stdout', text: `Сгенерировано форм: ${prepared.count}\n` });
        }
        if (typeof files['Cargo.toml'] !== 'string' || typeof files['src/main.rs'] !== 'string')
          throw Error('Нужны Cargo.toml и src/main.rs');
        const entries = Object.entries(files).filter(([file]) =>
          /^(Cargo\.(toml|lock)|src\/.+\.rs)$/.test(file),
        );
        if (entries.length > maxFileCount)
          throw Error(`Слишком много Rust-файлов (лимит ${maxFileCount})`);
        let totalLength = 0;
        for (const [file, source] of entries) {
          if (
            file.split('/').some(p => p === '..' || !p) ||
            file.includes('\\') ||
            typeof source !== 'string'
          )
            throw Error('Некорректный путь Rust-файла');
          totalLength += source.length;
          if (totalLength > maxTotalLength)
            throw Error(`Размер Rust-проекта превышает лимит ${maxTotalLength} символов`);
        }
        directory = await makeTemporaryDirectory(path.join(tmpdir(), 'forma-rust-'));
        for (const [file, source] of entries) {
          const destination = path.join(directory, file);
          await makeDirectory(path.dirname(destination), { recursive: true });
          await writeSourceFile(destination, source);
        }
        if (job.stopped) {
          if (release())
            send({ kind: 'finished', code: null, signal: 'SIGTERM', text: 'Завершено: SIGTERM' });
          return;
        }
        send({ kind: 'started', text: 'cargo run — ' + directory });
        const extraArgs = [
          ...(options.backend === 'egui' || options.backend === 'svet'
            ? ['--backend', options.backend]
            : []),
          ...(options.platform === 'ios' || options.platform === 'android'
            ? ['--platform', options.platform]
            : []),
        ];
        const args = [
          'run',
          '--offline',
          ...(options.native ? ['--features', 'native'] : []),
          '--manifest-path',
          path.join(directory, 'Cargo.toml'),
          ...(extraArgs.length ? ['--', ...extraArgs] : []),
        ];
        const env =
          options.backend || options.platform
            ? {
                ...process.env,
                ...(options.backend ? { FORMA_BACKEND: options.backend } : {}),
                ...(options.platform ? { FORMA_PLATFORM: options.platform } : {}),
              }
            : undefined;
        const proc = spawnProcess('cargo', args, {
          cwd: directory,
          detached: grouped,
          stdio: ['ignore', 'pipe', 'pipe'],
          ...(env ? { env } : {}),
        });
        child = proc;
        job.child = proc;
        proc.stdout.on('data', chunk => {
          if (!job.finished) send({ kind: 'stdout', text: chunk.toString() });
        });
        let errorBuffer = '';
        proc.stderr.on('data', chunk => {
          if (job.finished) return;
          errorBuffer += chunk.toString();
          const lines = errorBuffer.split('\n');
          errorBuffer = lines.pop();
          if (lines.length)
            send({ kind: 'stderr', text: mapFormDiagnostic(lines.join('\n') + '\n', sourceMap) });
        });
        proc.once('error', e => {
          if (!release()) return;
          send({ kind: 'error', text: e.message });
        });
        proc.once('close', (code, signal) => {
          if (!release()) return;
          if (errorBuffer)
            send({ kind: 'stderr', text: mapFormDiagnostic(errorBuffer, sourceMap) });
          send({ kind: 'finished', code, signal, text: `Завершено: ${signal ?? code}` });
        });
      } catch (e) {
        if (release()) send({ kind: 'error', text: e.message });
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
