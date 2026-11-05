// Compare generated WASM modules across font payload changes, including actual CPU frames.
const assert = require('node:assert/strict');
const {resolve} = require('node:path');
const {readFileSync, writeFileSync} = require('node:fs');
const {createHash} = require('node:crypto');
const [baselineDir, candidateDir, reportPath] = process.argv.slice(2);
assert(baselineDir && candidateDir && reportPath);
const baseline = require(resolve(baselineDir, 'forma.js'));
const candidate = require(resolve(candidateDir, 'forma.js'));
const rows = [];
for (const text of ['🙂', '😀❤🚀🌍', '你好 мир🙂', 'Latin🙂Ж', '漢字', '?']) {
  for (const size of [9, 14, 28, 48, 96]) {
    assert.deepEqual(Array.from(candidate.text_metrics(text, size)), Array.from(baseline.text_metrics(text, size)));
    assert.deepEqual(Array.from(candidate.shaped_text_metrics(text, size)), Array.from(baseline.shaped_text_metrics(text, size)));
    const files = JSON.stringify({'App.ui': `component App { Column { width:240; height:100; TextInput { width:220; height:70; fontSize:${size}; value:'${text}'; } } }`});
    assert.equal(candidate.Runtime.compile_ui_scene_artifact(files, 'App.ui', '{}'), baseline.Runtime.compile_ui_scene_artifact(files, 'App.ui', '{}'), 'public artifact JSON changed');
    for (const scale of [1, 1.25, 2]) {
      const snapshots = [baseline, candidate].map(api => {
        const view = new api.Runtime();
        try {
          view.load_ui_files_json(files, 'App.ui', '{}');
          view.focus_control(0);
          view.text_key('End', false, false);
          return {metrics: Array.from(view.text_caret_bounds()), pixels: Buffer.from(view.pixels(240, 100, scale))};
        } finally { view.free(); }
      });
      assert.deepEqual(snapshots[1], snapshots[0], `${text}, ${size}px, scale ${scale}`);
      rows.push({text, size, scale, pixels: snapshots[0].pixels.length, sha256: createHash('sha256').update(snapshots[0].pixels).digest('hex')});
    }
  }
}
const hash = dir => createHash('sha256').update(readFileSync(resolve(dir, 'forma_bg.wasm'))).digest('hex');
writeFileSync(reportPath, JSON.stringify({status:'passed', scope:'Public artifact JSON, simple/shaped metrics, mixed-font caret and CPU frame exact equality; six strings, five sizes, three scales. Not an OS presentation/full shaping gate.', baselineSha256:hash(baselineDir), candidateSha256:hash(candidateDir), rows}, null, 2)+'\n');
console.log(`${rows.length} frame comparisons passed`);
