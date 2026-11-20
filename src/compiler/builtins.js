// @ts-check
// What the compiler knows without a project: the primitive node types, the sources of the
// built-in components and the properties a base button falls back to.

import { visualTypes } from './vocabulary.js';

export const primitiveTypes = new Set([
  ...visualTypes,
  'ContentPresenter',
  'Brush',
  'Border',
  'Reveal',
  'PointerArea',
  'ContentText',
  'ContentShape',
  'ContentClip',
  'ContentClipEnd',
]);
// Scene-level text controls use the same primitive rendering as component
// content. A project may replace these defaults with components/Text*.ui.
export const builtinSources = {
  Text: `component Text {
    width: 260; height: 30; text: ''; fontSize: 16; color: #ffffff; wrap: false;
    background: #00000000; borderWidth: 0; radius: 0;
    Rectangle { background: props.background;
      Text { text: props.text; color: props.color; fontSize: props.fontSize; wrap: props.wrap; }
    }
  }`,
  TextInput: `component TextInput {
    width: 240; height: 38; value: ''; placeholder: ''; multiline: false;
    text: props.value; fontSize: 15; color: #ffffff; placeholderColor: #999999;
    background: #00000000; borderWidth: 0; radius: 0;
    Rectangle { background: props.background;
      TextInput { forward props { value, placeholder, multiline, fontSize, color, placeholderColor }; }
      PointerArea { clicked -> events.clicked(); }
    }
  }`,
  // The pressable area a container with a `clicked` handler lays under its children.
  ContainerArea: `component ContainerArea {
    width: 0; height: 0;
    Rectangle { background: #00000000; PointerArea { clicked -> events.clicked(); } }
  }`,
};
/** The component a container with a `clicked` handler lays down as its pressable area. */
export const containerArea = 'ContainerArea';
/** Built-in components the compiler places itself; a designer never inserts them by hand. */
export const internalComponents = new Set([containerArea]);
export const fallback = {
  width: 260,
  height: 70,
  text: '',
  fontSize: 20,
  color: { expr: '#14213b' },
  radius: 16,
  disabled: false,
  background: { expr: '#8ca5ff' },
  hoverBackground: { expr: '#a8baff' },
  pressedBackground: { expr: '#6a83da' },
  disabledBackground: { expr: '#596273' },
  borderWidth: 1,
  borderColor: { expr: '#bed0ff' },
  focusBorderColor: { expr: '#ffffff' },
  transitionDuration: { expr: '140ms' },
};
