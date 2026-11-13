// Undo and redo for operations that know how to reverse themselves: each has a label, an undo()
// and a redo(). A step that returns false did not happen, so the operation is dropped rather than
// moved to the other stack. Doing something new clears what could have been redone.
export function createOperationHistory({ limit = 50, announce = () => {} } = {}) {
  const done = [],
    undone = [];
  return {
    get canUndo() {
      return done.length > 0;
    },
    get canRedo() {
      return undone.length > 0;
    },
    push(operation) {
      done.push(operation);
      if (done.length > limit) done.shift();
      undone.length = 0;
    },
    async undo() {
      const operation = done.pop();
      if (!operation) return false;
      if ((await operation.undo()) !== false) {
        undone.push(operation);
        announce(`Отменено: ${operation.label}`);
      }
      return true;
    },
    async redo() {
      const operation = undone.pop();
      if (!operation) return false;
      if ((await operation.redo()) !== false) {
        done.push(operation);
        announce(`Повторено: ${operation.label}`);
      }
      return true;
    },
  };
}
