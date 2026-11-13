// The measuring callbacks a compile uses, with their answers remembered for that one compile.
import { parseSvgShapes, placeSvgShapes } from '../svg-shapes.js';

/**
 * Wraps the caller's metrics so that text is measured once per string and size, and an icon is
 * parsed and placed once per box and colour.
 *
 * @param {object} metrics the callbacks and options passed to the compiler
 */
export function cachingMetrics(metrics) {
  // Measurement and lowered SVG geometry are deterministic within one compile.
  // Keep these caches local so edits, state, fonts and metrics callbacks are
  // re-read on the next compilation, without retaining large output strings.
  const textSizes = new Map(),
    images = new Map(),
    parsedIcons = new Map(),
    measureText = metrics.measureText,
    metricsContext = metrics;
  // Parsed icon geometry is compile-local and bounded like the output strings below.
  const iconGeometryBudget = 1_000_000;
  let imageUnits = 0,
    iconGeometryUnits = 0;
  return {
    ...metrics,
    measureText: measureText
      ? function (text, size, family = 'sans') {
          // The default family keeps the bare size as its key and the two-argument call.
          const sizeKey = family === 'sans' ? size : `${family}:${size}`;
          let byText = textSizes.get(sizeKey);
          if (!byText) {
            byText = new Map();
            textSizes.set(sizeKey, byText);
          }
          if (!byText.has(text)) {
            const measured =
              family === 'sans'
                ? measureText.call(metricsContext, text, size)
                : measureText.call(metricsContext, text, size, family);
            byText.set(text, [measured[0], measured[1]]);
          }
          return byText.get(text);
        }
      : undefined,
    imageContent(source, box, currentColor) {
      let byBox = images.get(source);
      const key = `${box[0]},${box[1]},${box[2]},${box[3]},${currentColor}`;
      if (byBox?.has(key)) return byBox.get(key);
      const cached = parsedIcons.get(source);
      let parsed;
      if (cached?.color === currentColor) parsed = cached.parsed;
      else {
        parsed = parseSvgShapes(source, currentColor);
        if (iconGeometryUnits <= iconGeometryBudget) {
          parsedIcons.set(source, { color: currentColor, parsed });
          iconGeometryUnits += parsed.shapes.reduce(
            (total, shape) => total + shape.points.length * 16,
            64,
          );
        }
      }
      const content = placeSvgShapes(parsed, box)
        .map(s => {
          let pts = '';
          for (let i = 0; i < s.points.length; i++)
            pts += (i ? ' ' : '') + s.points[i][0] + ' ' + s.points[i][1];
          return `ContentShape { points: '${pts}'; color: ${s.color}; }`;
        })
        .join(' ');
      if (imageUnits + content.length <= 1_000_000) {
        if (!byBox) {
          byBox = new Map();
          images.set(source, byBox);
        }
        byBox.set(key, content);
        imageUnits += content.length;
      }
      return content;
    },
  };
}
