import { type ChildNodeValue, type Component, Fragment, h } from './dom';
import { type Signal } from './reactivity';

export { Fragment };

export function jsx(
  type: string | Component<any>,
  props: Record<string, any> | null,
  key?: string | number
): Node {
  const finalProps = props ? { ...props } : {};
  if (key !== undefined) {
    finalProps.key = key;
  }
  const children = finalProps.children;
  delete finalProps.children;
  if (children !== undefined) {
    return h(type, finalProps, ...(Array.isArray(children) ? children : [children]));
  }
  return h(type, finalProps);
}

export const jsxs = jsx;
export const jsxDEV = jsx;

export namespace JSX {
  export type Element = Node;
  export interface ElementChildrenAttribute {
    children: {};
  }
  export interface IntrinsicElements {
    [elemName: string]: Record<string, any> & {
      children?: ChildNodeValue;
      class?: string | (() => string);
      className?: string | (() => string);
      style?: string | Record<string, any> | (() => string | Record<string, any>);
      model?: Signal<any>;
    };
  }
}
