/**
 * # Forma 3D tags (`svet-engine/forma-3d`)
 *
 * Side-effect module: registers `Scene3D`, `OrbitCube3D` and `Viewport3D` with the Forma runtime.
 * It is the only link between Forma and `gpu.ts`, so a `.ui` application without 3D widgets that uses
 * `svet-engine/slim` does not carry the GPU module. The default `svet-engine` entry imports it.
 */
import { Canvas3D, Viewport3D, createCubeMesh3D, createPerspectiveCamera3D, createTransform3D } from './gpu.ts';
import { effect, onCleanup } from './reactivity.ts';
import { evalAstNode, evaluateFormaExpression, parseFormaExprAst } from './forma-expr.ts';
import { registerFormaElement, type FormaElementContext } from './forma.ts';

/** Declarative 3D widget: `Scene3D { rotX: ...; rotY: ...; rotZ: ...; scale: ...; width: ...; height: ...; }` */
function renderForma3d({ tag, props, scope, registerKey }: FormaElementContext): Node {
  const widthAst = props.width ? parseFormaExprAst(props.width) : null;
  const heightAst = props.height ? parseFormaExprAst(props.height) : null;
  const rotXAst = props.rotX ? parseFormaExprAst(props.rotX) : null;
  const rotYAst = props.rotY ? parseFormaExprAst(props.rotY) : null;
  const rotZAst = props.rotZ ? parseFormaExprAst(props.rotZ) : null;
  const scaleAst = props.scale ? parseFormaExprAst(props.scale) : null;
  const width = () => (widthAst ? Number(evalAstNode(widthAst, scope)) || 320 : 320);
  const height = () => (heightAst ? Number(evalAstNode(heightAst, scope)) || 180 : 180);
  const transform = createTransform3D({
    position: [0, 0, -3.2],
    rotation: [0.4, 0.6, 0.1],
    scale: [1, 1, 1],
  });
  const camera = createPerspectiveCamera3D({
    fovYRad: Math.PI / 3,
    aspect: width() / Math.max(1, height()),
  });
  effect(() => camera.aspect.set(width() / Math.max(1, height())));
  const cube = createCubeMesh3D({ size: 1.35, transform, camera });
  onCleanup(() => {
    cube.mesh.dispose();
    camera.dispose();
    transform.dispose();
  });

  effect(() => {
    const rx = rotXAst ? Number(evalAstNode(rotXAst, scope)) || 0 : 0.4;
    const ry = rotYAst ? Number(evalAstNode(rotYAst, scope)) || 0 : 0.6;
    const rz = rotZAst ? Number(evalAstNode(rotZAst, scope)) || 0 : 0.1;
    transform.rotation.set([rx, ry, rz]);
  });

  if (scaleAst) {
    effect(() => {
      const s = Number(evalAstNode(scaleAst, scope)) || 1;
      transform.scale.set([s, s, s]);
    });
  }

  const canvas = (tag === 'Viewport3D' ? Viewport3D : Canvas3D)({
    renderMode: props.renderMode ? String(evaluateFormaExpression(props.renderMode, scope)) as 'sync' | 'animation-frame' : undefined,
    mesh: cube.mesh,
    edges: cube.edges,
    faces: cube.faces,
    width,
    height,
    orbitTransform: transform,
    strokeStyle: props.strokeColor
      ? String(evaluateFormaExpression(props.strokeColor, scope))
      : '#38bdf8',
  });
  canvas.setAttribute('data-forma-type', tag);
  if (props.key) {
    const keyVal = String(evaluateFormaExpression(props.key, scope));
    canvas.setAttribute('data-forma-key', keyVal);
    registerKey(keyVal, canvas);
  }
  return canvas;
}

registerFormaElement(['Scene3D', 'OrbitCube3D', 'Viewport3D'], renderForma3d);
