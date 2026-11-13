// The property names the component compiler knows: for every control, for layout, and for each
// kind of content a control can draw.
import { containerTypes } from '../component-layout.js';

export const standard = new Set(
  'key x y width height text fontSize font.size color radius disabled background hoverBackground pressedBackground disabledBackground borderWidth borderColor focusBorderColor transitionDuration transition'.split(
    ' ',
  ),
);
export const layoutProps = new Set(
  'key cell row column row.span column.span width height minWidth maxWidth minHeight maxHeight transition'.split(
    ' ',
  ),
);
export const visualTypes = new Set([...containerTypes, 'Text', 'TextInput', 'Image', 'Rectangle']);
export const contentProps = new Map(
  Object.entries({
    Frame: ['columns', 'rows', 'gap', 'padding', 'clip', 'radius'],
    Row: ['gap', 'padding', 'clip', 'radius'],
    Column: ['gap', 'padding', 'clip', 'radius'],
    Grid: ['columns', 'rows', 'gap', 'padding', 'clip', 'radius'],
    Stack: ['gap', 'padding', 'clip', 'radius'],
    Rectangle: ['background', 'radius'],
    TextInput: ['value', 'placeholder', 'color', 'placeholderColor', 'fontSize', 'multiline'],
    Text: ['text', 'color', 'fontSize', 'font.size', 'fontFamily', 'font.family', 'wrap'],
    Image: ['source', 'color'],
  }).map(([type, props]) => [type, new Set([...layoutProps, ...props])]),
);
