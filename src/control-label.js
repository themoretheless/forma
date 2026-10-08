// The one place that knows which leading glyphs decorate a Studio control's text: the command
// palette lists controls by this label and the Rust-drawn shell rasterizes it.
export const glyphPrefix=/^[▶⌖↻↺☷◐◇↑↓✓●]+\s*/u;

export function collapseText(text){return String(text??'').replace(/\s+/g,' ').trim();}

// Visible text without its decorative glyph, as a label a person reads or a palette searches.
export function controlLabel(el){return collapseText(el.getAttribute?.('aria-label')||el.textContent||el.title).replace(glyphPrefix,'').trim();}
