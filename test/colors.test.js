import {test} from 'node:test';
import assert from 'node:assert/strict';
import {colorValue} from '../src/color-swatches.js';
test('swatch colors support literal and quoted formats and alpha conversion',()=>{
 assert.equal(colorValue("'#FF804080'"),'#FF804080');
 assert.equal(colorValue('rgb(255 128 64 / 50%)'),'rgb(255 128 64 / 50%)');
 assert.equal(colorValue('oklch(70% 0.16 45)'),'oklch(70% 0.16 45)');
 assert.equal(colorValue('argb(0, 255, 128, 64)'),'rgb(255 128 64 / 0)');
 assert.equal(colorValue('argb(300, 0, 0, 0)'),null);
 assert.equal(colorValue('state.color'),null);
});
