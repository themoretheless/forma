/** @vitest-environment jsdom */
import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  compileFormaComponent,
  getGraphTelemetry,
  h,
  parseFormaSource,
  resetReactiveSystem,
} from '../src/engine';

const topbarUi = `component Topbar {
    tokens { gap: "6px"; }
    Row {
        class: "topbar-left"; gap: tokens.gap;
        Slot { name: "fileMenu"; }
        if state.workspaceConflict {
            Row {
                class: "persistence-conflict";
                Text { text: state.conflictWarning; }
                Button { text: state.exportDraftLabel; clicked -> actions.saveSource(); }
                Button { text: state.useIndexedLabel; clicked -> actions.resolveConflict(false); }
                Button { text: state.restoreDraftLabel; clicked -> actions.resolveConflict(true); }
            }
        }
        if state.persistenceError {
            Button { class: "persistence-status persistence-error"; text: state.unsavedDraftLabel; title: state.persistenceErrorTitle; clicked -> actions.retryPersistence(); }
        }
    }
    Row {
        class: "mode-switch"; role: "group"; ariaLabel: state.workspaceModeGroupLabel;
        for mode in state.modes key mode.id {
            SegmentButton { text: mode.label; title: mode.hint; active: state.workspaceMode === mode.id; ariaPressed: state.workspaceMode === mode.id; clicked -> actions.openWorkspaceMode(mode.id); }
        }
    }
    Row {
        class: "topbar-right"; gap: tokens.gap;
        Slot { name: "generators"; }
        Badge { class: "kernel-badge"; text: "Manifold · Forma"; }
        IconButton { class: "source-toggle"; active: state.editorOpen; ariaPressed: state.editorOpen; ariaLabel: state.sourceLabel; title: state.sourceLabel; text: "</>"; clicked -> actions.toggleEditor(); }
        Button { class: "command-btn"; ariaLabel: state.commandsLabel; title: state.commandHelp; clicked -> actions.openCommandPalette(); SvgIcon { path: "m21 21-4.5-4.5 M18 10a8 8 0 1 1-16 0a8 8 0 0 1 16 0"; } Span { text: state.commandsLabel; } Keycap { text: "⌘K"; } }
        PrimaryButton { text: state.exportLabel; title: state.exportTitle; clicked -> actions.openExportDialog(); }
        Slot { name: "settingsMenu"; }
    }
}`;

const editorToolbarUi = `component EditorToolbar {
    tokens { gap: "8px"; }
    Row {
        class: "editor-toolbar"; gap: tokens.gap;
        PrimaryButton { text: state.renderLabel; disabled: state.rendering; clicked -> actions.renderFull(); }
        if state.rendering { Button { text: state.cancelBuildLabel; clicked -> actions.cancelGeometryBuild(); } }
        Checkbox { checked <-> state.autoRender; label: state.autoLabel; }
        if state.solidBuilding { Button { text: state.cancelSolidLabel; clicked -> actions.cancelSolidBuild(); } }
        Spacer {}
        Slot { name: "vr"; }
        Slot { name: "functionReference"; }
        Slot { name: "moreMenu"; }
    }
}`;

const editorStatsUi = `component EditorStats {
    tokens { gap: "12px"; }
    Row {
        class: "stats"; gap: tokens.gap;
        Text { text: state.meshesLabel; } Strong { text: state.meshesValue; }
        Text { text: state.trianglesLabel; } Strong { text: state.trianglesValue; }
        if state.hasMeshes {
            Text { text: state.volumeLabel; } Strong { text: state.volumeValue; }
            Text { text: state.areaLabel; } Strong { text: state.areaValue; }
        }
        Text { class: state.statusClass; role: "status"; text: state.statusSummary; }
    }
}`;

const viewerToolbarUi = `component ViewerToolbar {
    tokens { gap: "4px"; }
    Row {
        class: state.toolbarClass; gap: tokens.gap;
        IconButton { class: "view-btn icon-only"; ariaLabel: state.fitLabel; title: state.fitLabel; text: "⊡"; clicked -> actions.fitView(); }
        IconButton { class: "view-btn icon-only"; ariaLabel: state.resetLabel; title: state.resetLabel; text: "↺"; clicked -> actions.resetView(); }
        IconButton { class: "view-btn icon-only"; ariaLabel: state.previousViewLabel; title: state.previousViewTitle; disabled: !state.canPreviousView; text: "←"; clicked -> actions.previousView(); }
        Button { class: "view-btn"; text: state.projectionLabel; title: state.projectionTitle; ariaPressed: state.isOrthographic; clicked -> actions.toggleProjection(); }
        Button { class: "view-btn"; text: state.gridLabel; active: state.gridVisible; ariaPressed: state.gridVisible; clicked -> actions.toggleGrid(); }
        Slot { name: "gridAndMaterial"; }
        Divider {}
        Row {
            class: "display-modes"; role: "group"; ariaLabel: state.displayLabel;
            SegmentButton { class: "btn view-btn"; text: state.shadedLabel; active: state.displayMode === "shaded"; ariaPressed: state.displayMode === "shaded"; clicked -> actions.setDisplayMode("shaded"); }
            SegmentButton { class: "btn view-btn"; text: state.edgesLabel; active: state.displayMode === "edges"; ariaPressed: state.displayMode === "edges"; clicked -> actions.setDisplayMode("edges"); }
            SegmentButton { class: "btn view-btn"; text: state.xrayLabel; active: state.displayMode === "xray"; ariaPressed: state.displayMode === "xray"; clicked -> actions.setDisplayMode("xray"); }
        }
    }
}`;

const selectionModesUi = `component SelectionModes {
    tokens { gap: "6px"; }
    Column {
        class: "selection-modes"; role: "group"; ariaLabel: state.selectionModeLabel;
        IconButton { ariaLabel: state.pointLabel; title: state.pointTitle; active: state.selectionMode === "point"; ariaPressed: state.selectionMode === "point"; clicked -> actions.setSelectionMode("point"); Span { class: "mode-point"; } }
        IconButton { ariaLabel: state.faceLabel; title: state.faceTitle; active: state.selectionMode === "face"; ariaPressed: state.selectionMode === "face"; clicked -> actions.setSelectionMode("face"); Span { class: "mode-face"; } }
        IconButton { ariaLabel: state.bodyLabel; title: state.bodyTitle; active: state.selectionMode === "object"; ariaPressed: state.selectionMode === "object"; clicked -> actions.setSelectionMode("object"); Span { class: "mode-body"; } }
    }
    if state.showHud {
        Row {
            class: "selection-hud"; gap: tokens.gap;
            Text { text: state.selectedSummary; }
            if state.hasSelectedMesh { Button { text: state.focusLabel; clicked -> actions.focusSelection(); } }
            Button { text: state.isolateLabel; title: state.isolateTitle; active: state.isolated; ariaPressed: state.isolated; disabled: !state.hasSelectedMesh && !state.isolated; clicked -> actions.toggleIsolate(); }
            if state.hasSelectedMesh { Button { text: state.deselectLabel; clicked -> actions.clearSelection(); } }
        }
    }
}`;

const dockRailUi = `component DockRail {
    tokens { gap: "4px"; }
    Column {
        class: "dock-rail"; role: "tablist"; ariaLabel: state.sidebarLabel; ariaOrientation: "vertical"; gap: tokens.gap;
        for tab in state.tabs key tab.id {
            IconButton { id: tab.domId; role: "tab"; ariaLabel: tab.label; title: tab.label; active: state.dockTab === tab.id; ariaSelected: state.dockTab === tab.id; ariaControls: "dock-panel"; tabindex: state.dockTab === tab.id ? 0 : -1; clicked -> actions.selectDockTab(tab.id); keydown -> actions.dockTabKeydown(); SvgIcon { path: tab.iconPath; } }
        }
        Spacer { class: "dock-rail-spacer"; }
        IconButton { class: "dock-close"; ariaLabel: state.closeSidebarLabel; title: state.closeSidebarLabel; text: "×"; clicked -> actions.closeDock(); }
    }
}`;

const computeInspectorUi = `component ComputeInspector {
    tokens { gap: "10px"; }
    Column {
        class: "compute-inspector"; gap: tokens.gap; padding: 12;
        Strong { text: state.panelTitle; }
        Paragraph { text: state.description; }
        PrimaryButton { text: state.computeButtonLabel; disabled: !state.canRunCompute; clicked -> actions.runGeometryAnalysis(); }
        if state.computeBusy { Button { text: state.cancelLabel; clicked -> actions.cancelGeometryAnalysis(); } }
        if state.hasResult {
            Text { text: state.areaPrefix + state.areaValue; }
            Text { text: state.backendSummary; }
            Text { text: state.timingSummary; }
        }
        if state.showFallback { Paragraph { text: state.fallbackNote; } }
        Paragraph { text: state.verificationNote; }
        if state.computeError { Paragraph { role: "alert"; text: state.computeError; } }
    }
}`;

const statusbarUi = `component Statusbar {
    tokens { gap: "14px"; }
    Footer {
        class: "statusbar"; ariaLabel: state.statusAriaLabel; gap: tokens.gap;
        Span { class: "status-item"; StatusDot { class: state.gpuDotClass; } Text { text: state.backendLabel; } }
        Text { class: "kernel-badge"; text: "Manifold · Forma"; }
        if state.showSortedTransparency { Text { text: state.transparencySortedLabel; } }
        if state.showPersistence { Text { text: state.persistenceLabel; } }
        if state.conversionPending {
            Text { text: state.conversionLabel; }
            ProgressBar { max: state.conversionMax; value: state.conversionValue; ariaLabel: state.conversionLabel; }
            Button { text: state.cancelLabel; clicked -> actions.cancelConversion(); }
        }
        Text { class: "status-hint"; text: state.hintLabel; }
    }
}`;

const workspaceActionBarUi = `component WorkspaceActionBar {
    tokens { gap: "8px"; }
    Row {
        class: "workspace-action-bar"; gap: tokens.gap;
        Slot {
            name: "default";
            Button { text: state.cubeLabel || "+ Куб"; disabled: state.busy; clicked -> actions.insertPrimitive("cube"); }
            Button { text: state.cylinderLabel || "+ Цилиндр"; disabled: state.busy; clicked -> actions.insertPrimitive("cylinder"); }
            Button { text: state.sphereLabel || "+ Сфера"; disabled: state.busy; clicked -> actions.insertPrimitive("sphere"); }
            PrimaryButton { text: state.booleanLabel || "Вычитание CSG"; disabled: state.busy; clicked -> actions.applyBoolean("difference"); }
        }
    }
}`;

beforeEach(() => resetReactiveSystem());

describe('Forma CAD UI regions (open-scad-viewer parity & reactivity)', () => {
  it('parses and mounts all 9 CAD .ui regions with zero leaked WASM reactive nodes on dispose', () => {
    for (const src of [
      topbarUi,
      editorToolbarUi,
      editorStatsUi,
      viewerToolbarUi,
      selectionModesUi,
      dockRailUi,
      computeInspectorUi,
      statusbarUi,
      workspaceActionBarUi,
    ]) {
      const ast = parseFormaSource(src);
      expect(ast.rootAstNodes.length).toBeGreaterThan(0);
      const container = document.createElement('div');
      const inst = compileFormaComponent(ast);
      const handle = inst.mount(container);
      expect(container.childNodes.length).toBeGreaterThan(0);
      handle.unmount();
      inst.dispose();
      expect(getGraphTelemetry().nodes).toBe(0);
    }
  });

  it('supports Topbar.ui slots, dynamic state.setState(), SvgIcon + Keycap children, and custom action dispatch', () => {
    const onAction = vi.fn();
    const container = document.createElement('div');
    const inst = compileFormaComponent(topbarUi, {
      state: {
        workspaceModeGroupLabel: 'Режим работы',
        workspaceMode: 'solid',
        modes: [
          { id: 'solid', label: 'Solid', hint: 'Точные тела' },
          { id: 'mesh', label: 'Mesh', hint: 'Полигональная сетка' },
        ],
        workspaceConflict: false,
        persistenceError: false,
        editorOpen: false,
        sourceLabel: 'Исходный код',
        commandHelp: 'Команды',
        commandsLabel: 'Команды',
        exportTitle: 'Экспорт модели',
        exportLabel: 'Экспорт',
      },
      slots: {
        namedSlots: {
          fileMenu: () => h('mark', { id: 'file-slot' }, 'File menu'),
          settingsMenu: () => h('mark', { id: 'settings-slot' }, 'Settings'),
        },
      },
      onAction,
    });
    const handle = inst.mount(container);
    try {
      expect(container.querySelectorAll('mark')).toHaveLength(2);
      const segButtons = () => Array.from(container.querySelectorAll<HTMLButtonElement>('.forma-seg-btn'));
      expect(segButtons().map((b) => b.getAttribute('aria-pressed'))).toEqual(['true', 'false']);
      expect(segButtons()[0].classList.contains('active')).toBe(true);
      expect(segButtons()[1].classList.contains('active')).toBe(false);

      const cmdBtn = container.querySelector<HTMLButtonElement>('.command-btn')!;
      expect(cmdBtn.querySelector('svg')).not.toBeNull();
      expect(cmdBtn.querySelector('kbd')?.textContent).toBe('⌘K');

      // Update state reactively via setState()
      inst.setState({ workspaceMode: 'mesh', editorOpen: true });
      expect(segButtons().map((b) => b.getAttribute('aria-pressed'))).toEqual(['false', 'true']);
      expect(segButtons()[0].classList.contains('active')).toBe(false);
      expect(segButtons()[1].classList.contains('active')).toBe(true);

      const sourceToggle = container.querySelector<HTMLButtonElement>('.source-toggle')!;
      expect(sourceToggle.getAttribute('aria-pressed')).toBe('true');
      expect(sourceToggle.classList.contains('active')).toBe(true);
      sourceToggle.click();
      expect(onAction).toHaveBeenCalledWith('toggleEditor', [], expect.any(Event));

      // Append new mode and click it
      inst.setState({
        modes: [
          { id: 'solid', label: 'Solid', hint: 'Точные тела' },
          { id: 'mesh', label: 'Mesh', hint: 'Полигональная сетка' },
          { id: 'code', label: 'Code', hint: 'Код' },
        ],
      });
      expect(segButtons()).toHaveLength(3);
      segButtons()[2].click();
      expect(onAction).toHaveBeenLastCalledWith('openWorkspaceMode', ['code'], expect.any(Event));
    } finally {
      handle.unmount();
      inst.dispose();
    }
    expect(getGraphTelemetry().nodes).toBe(0);
  });

  it('supports EditorToolbar.ui Checkbox two-way binding (checked <-> state.autoRender), DockRail keydown, and Statusbar ProgressBar', () => {
    const onAction = vi.fn();
    const onBinding = vi.fn();
    const container = document.createElement('div');
    const toolbar = compileFormaComponent(editorToolbarUi, {
      state: {
        rendering: false,
        renderLabel: 'Собрать',
        cancelBuildLabel: 'Отменить сборку',
        autoRender: true,
        autoLabel: 'Авто',
        solidBuilding: false,
        cancelSolidLabel: 'Отмена',
      },
      onAction,
      onBinding,
    });
    const hToolbar = toolbar.mount(container);
    try {
      const chk = container.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
      expect(chk).not.toBeNull();
      expect(chk.checked).toBe(true);
      expect(container.querySelector('.forma-checkbox')?.textContent).toContain('Авто');

      chk.checked = false;
      chk.dispatchEvent(new Event('change', { bubbles: true }));
      expect(toolbar.state.autoRender()).toBe(false);
      expect(onBinding).toHaveBeenCalledWith('autoRender', false);
    } finally {
      hToolbar.unmount();
      toolbar.dispose();
    }

    const dockAction = vi.fn();
    const dock = compileFormaComponent(dockRailUi, {
      state: {
        sidebarLabel: 'Боковая панель',
        dockTab: 'scene',
        tabs: [
          { id: 'scene', domId: 'tab-scene', label: 'Сцена', iconPath: 'M1 1' },
          { id: 'params', domId: 'tab-params', label: 'Параметры', iconPath: 'M2 2' },
        ],
        closeSidebarLabel: 'Закрыть',
      },
      onAction: dockAction,
    });
    const hDock = dock.mount(container);
    try {
      const tabScene = container.querySelector<HTMLButtonElement>('#tab-scene')!;
      const tabParams = container.querySelector<HTMLButtonElement>('#tab-params')!;
      expect(tabScene.getAttribute('tabindex')).toBe('0');
      expect(tabParams.getAttribute('tabindex')).toBe('-1');
      tabScene.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
      expect(dockAction).toHaveBeenCalledWith('dockTabKeydown', [], expect.any(KeyboardEvent));
    } finally {
      hDock.unmount();
      dock.dispose();
    }

    const statusAction = vi.fn();
    const status = compileFormaComponent(statusbarUi, {
      state: {
        conversionPending: true,
        conversionLabel: 'Exporting',
        conversionValue: 1,
        conversionMax: 3,
        cancelLabel: 'Stop',
      },
      onAction: statusAction,
    });
    const hStatus = status.mount(container);
    try {
      const prog = container.querySelector<HTMLProgressElement>('progress')!;
      expect(prog.getAttribute('value')).toBe('1');
      expect(prog.getAttribute('max')).toBe('3');
      instClickByText(container, 'Stop');
      expect(statusAction).toHaveBeenCalledWith('cancelConversion', [], expect.any(Event));
    } finally {
      hStatus.unmount();
      status.dispose();
    }
    expect(getGraphTelemetry().nodes).toBe(0);
  });
});

function instClickByText(root: HTMLElement, text: string) {
  const btn = Array.from(root.querySelectorAll('button')).find((b) => b.textContent === text);
  btn?.click();
}
