# Executable language example

From the repository root:

```sh
cargo run --offline --manifest-path vector-ui/examples/language-app/Cargo.toml
```

This headless program generates its Rust form during the build, edits a task,
reorders the collection while preserving focus, removes a task through a typed
event, toggles conditional content, and renders a frame. Assertions verify the
model and control identity. It needs Node.js for generation and no window server.

`TaskButton.ui` demonstrates required properties, an enum, one grouped `match`
and explicit `forward`. `TaskList.ui` combines live interpolation, `if`, keyed
`for`, two-way row editing, typed event arguments, and Row/Grid layout.
