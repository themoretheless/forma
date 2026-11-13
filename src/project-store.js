// Where the open project is kept: always in the browser, and in a folder on disk when one is
// attached. It owns no DOM; the editor is told the outcome through `status`.

/**
 * @param {object} ports
 * @param {string} ports.key browser storage key of the project
 * @param {(key: string, files: object) => boolean} ports.writeProject
 * @param {(handle: object, files: object, previous: object) => Promise<void>} ports.writeDirectoryProject
 * @param {(status: 'saved' | 'unsaved', text: string) => void} ports.status
 */
export function createProjectStore({ key, writeProject, writeDirectoryProject, status }) {
  let directory = null,
    // What the folder holds, so a save writes only the files that changed.
    saved = {},
    // Folder writes run one after another. Two overlapping writes of the same file could finish
    // in either order and leave the older text on disk under a "saved" status.
    writes = Promise.resolve();

  const enqueue = work => {
    const run = writes.then(work);
    writes = run.catch(() => {});
    return run;
  };

  return {
    /** The attached folder handle, or `null`. */
    get directory() {
      return directory;
    },
    /** Saves to `handle` from now on; `files` is what the folder already holds. */
    attach(handle, files) {
      directory = handle;
      saved = { ...files };
    },
    detach() {
      directory = null;
      saved = {};
    },
    /**
     * Saves `files`. The browser copy is written at once; the returned promise settles when the
     * folder copy, if any, has been written or given up on.
     */
    save(files) {
      try {
        if (!writeProject(key, files)) throw Error('Storage unavailable');
      } catch {
        status('unsaved', 'Не сохранено: экспортируйте проект');
        return Promise.resolve();
      }
      if (!directory) {
        status('saved', 'Сохранено');
        return Promise.resolve();
      }
      const handle = directory,
        snapshot = { ...files };
      return enqueue(async () => {
        // The folder was detached or replaced while this save waited its turn.
        if (directory !== handle) return;
        try {
          await writeDirectoryProject(handle, snapshot, saved);
          if (directory === handle) saved = snapshot;
          status('saved', `Сохранено (${handle.name})`);
        } catch {
          if (directory === handle) directory = null;
          status('saved', 'Сохранено в браузере (доступ к папке потерян)');
        }
      });
    },
    /** Writes `files` to the attached folder as its whole content; rejects if that fails. */
    writeToDirectory(files) {
      const handle = directory;
      return enqueue(async () => {
        await writeDirectoryProject(handle, files, saved);
        if (directory === handle) saved = { ...files };
      });
    },
  };
}
