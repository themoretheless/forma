use super::*;

#[test]
fn binary_and_tristate_checks_have_explicit_cycles() {
    let mut binary = CheckModel::new(CheckState::Mixed, CheckMode::Binary);
    binary.activate();
    assert_eq!(binary.state(), CheckState::Checked);
    binary.activate();
    assert_eq!(binary.state(), CheckState::Unchecked);
    let mut tri = CheckModel::new(CheckState::Unchecked, CheckMode::TriState);
    for expected in [
        CheckState::Checked,
        CheckState::Mixed,
        CheckState::Unchecked,
    ] {
        assert!(tri.activate());
        assert_eq!(tri.state(), expected);
    }
    let mut toggle = ToggleModel::default();
    assert!(!toggle.set_value(false));
    assert!(toggle.activate());
    assert!(toggle.value());
}

#[test]
fn range_snaps_clamps_and_reaches_non_grid_endpoints() {
    let mut range = RangeModel::new(0.0, 10.0, 3.0, 4.0).unwrap();
    assert_eq!(range.value(), 3.0);
    range.step_by(2);
    assert_eq!(range.value(), 9.0);
    range.step_by(1);
    assert_eq!(range.value(), 10.0);
    assert!(!range.step_by(1));
    range.step_by(-1);
    assert_eq!(range.value(), 9.0);
    range.step_by(i64::MIN);
    assert_eq!(range.value(), 0.0);
    range.step_by(i64::MAX);
    assert_eq!(range.value(), 10.0);
    range.set_fraction(-2.0).unwrap();
    assert_eq!(range.fraction(), 0.0);
    range.set_fraction(2.0).unwrap();
    assert_eq!(range.fraction(), 1.0);
}

#[test]
fn decimal_steps_do_not_skip_and_invalid_updates_are_atomic() {
    let mut range = RangeModel::new(-1.0, 1.0, 0.1, -0.7).unwrap();
    range.step_by(1);
    assert!((range.value() + 0.6).abs() < 1e-12);
    range.step_by(-1);
    assert!((range.value() + 0.7).abs() < 1e-12);
    let before = range;
    assert!(range.set_value(f64::NAN).is_err());
    assert_eq!(range, before);
    assert!(range.set_fraction(f64::INFINITY).is_err());
    assert_eq!(range, before);
    for (min, max, step) in [
        (1.0, 0.0, 1.0),
        (0.0, 1.0, 0.0),
        (0.0, 1.0, -1.0),
        (0.0, f64::INFINITY, 1.0),
        (-f64::MAX, f64::MAX, 1.0),
    ] {
        assert!(RangeModel::new(min, max, step, 0.0).is_err());
    }
    let mut fixed = RangeModel::new(4.0, 4.0, 1.0, 9.0).unwrap();
    assert_eq!(fixed.value(), 4.0);
    assert_eq!(fixed.fraction(), 0.0);
    assert!(!fixed.step_by(1));
}

#[test]
fn exclusive_selection_uses_ids_and_wraps_or_clamps() {
    let mut radio = RadioGroup::new(vec![10, 30, 50], None).unwrap();
    assert!(radio.move_by(-1, false));
    assert_eq!(radio.selected(), Some(50));
    assert!(!radio.move_by(1, false));
    assert!(radio.move_by(1, true));
    assert_eq!(radio.selected(), Some(10));
    radio.select(30).unwrap();
    assert!(!radio.select(30).unwrap());
    assert!(radio.select(999).is_err());
    assert_eq!(radio.selected(), Some(30));
    radio.replace_items(vec![50, 30]).unwrap();
    assert_eq!(radio.selected(), Some(30));
    assert!(radio.replace_items(vec![1, 1]).is_err());
    assert_eq!(radio.items(), &[50, 30]);
    radio.replace_items(vec![50]).unwrap();
    assert_eq!(radio.selected(), None);
    let mut empty = TabSelection::new(vec![], None).unwrap();
    assert!(!empty.first());
    assert!(!empty.last());
    assert!(!empty.move_by(isize::MAX, true));
}

fn tree() -> TreeModel {
    TreeModel::new(vec![
        TreeItem {
            id: 1,
            parent: None,
        },
        TreeItem {
            id: 2,
            parent: Some(1),
        },
        TreeItem {
            id: 3,
            parent: Some(2),
        },
        TreeItem {
            id: 4,
            parent: Some(1),
        },
        TreeItem {
            id: 5,
            parent: None,
        },
    ])
    .unwrap()
}
#[test]
fn tree_navigation_visits_visible_rows_and_collapse_preserves_visible_selection() {
    let mut tree = tree();
    assert_eq!(tree.visible_ids(), vec![1, 5]);
    assert!(tree.select(3).is_err());
    tree.right();
    assert_eq!(tree.selected(), Some(1));
    tree.right();
    assert!(tree.is_expanded(1));
    tree.right();
    assert_eq!(tree.selected(), Some(2));
    tree.right();
    tree.right();
    assert_eq!(tree.selected(), Some(3));
    assert_eq!(tree.visible_ids(), vec![1, 2, 3, 4, 5]);
    tree.set_expanded(1, false).unwrap();
    assert_eq!(tree.selected(), Some(1));
    tree.move_by(1);
    assert_eq!(tree.selected(), Some(5));
    assert!(!tree.left());
    tree.select(1).unwrap();
    tree.right();
    tree.right();
    assert_eq!(tree.selected(), Some(2));
    tree.left();
    assert!(!tree.is_expanded(2));
    tree.left();
    assert_eq!(tree.selected(), Some(1));
}
#[test]
fn malformed_tree_order_and_duplicate_ids_are_rejected() {
    assert!(
        TreeModel::new(vec![TreeItem {
            id: 1,
            parent: Some(1)
        }])
        .is_err()
    );
    assert!(
        TreeModel::new(vec![
            TreeItem {
                id: 1,
                parent: None
            },
            TreeItem {
                id: 1,
                parent: None
            }
        ])
        .is_err()
    );
    assert!(
        TreeModel::new(vec![
            TreeItem {
                id: 1,
                parent: None
            },
            TreeItem {
                id: 2,
                parent: None
            },
            TreeItem {
                id: 3,
                parent: Some(1)
            }
        ])
        .is_err()
    );
}

#[test]
fn text_caret_selection_and_deletion_obey_utf8_boundaries() {
    let mut edit = TextEdit::new("Aя🦀e\u{301}".into()).unwrap();
    edit.home(false);
    edit.right(false);
    assert_eq!(edit.caret(), 1);
    edit.right(false);
    assert_eq!(edit.caret(), 3);
    edit.right(true);
    assert_eq!(edit.selected_text(), "🦀");
    edit.insert("中").unwrap();
    assert_eq!(edit.value(), "Aя中e\u{301}");
    assert_eq!(edit.caret(), 6);
    edit.backspace();
    assert_eq!(edit.value(), "Aяe\u{301}");
    edit.delete();
    assert_eq!(edit.value(), "Aя\u{301}");
    let before = edit.current.clone();
    assert!(edit.set_selection(2, 3).is_err());
    assert_eq!(edit.current, before);
    assert!(edit.set_selection(0, 999).is_err());
    assert_eq!(edit.current, before);
    assert!(edit.insert("bad\nline").is_err());
    assert_eq!(edit.current, before);
}

#[test]
fn text_selection_collapses_and_home_end_extend_from_anchor() {
    let mut edit = TextEdit::new("абв".into()).unwrap();
    edit.set_selection(2, 6).unwrap();
    edit.left(false);
    assert_eq!(edit.selection(), 2..2);
    edit.end(true);
    assert_eq!(edit.selected_text(), "бв");
    edit.home(true);
    assert_eq!(edit.selected_text(), "а");
    assert_eq!(edit.anchor(), 2);
    edit.right(false);
    assert_eq!(edit.selection(), 2..2);
    edit.select_all();
    edit.delete();
    assert_eq!(edit.value(), "");
    assert!(!edit.backspace());
    assert!(!edit.delete());
    assert!(!edit.left(false));
    assert!(!edit.right(false));
}

#[test]
fn text_moves_multiple_unicode_scalars_and_clamps_large_deltas() {
    let mut edit = TextEdit::new("Aя🦀中".into()).unwrap();
    edit.home(false);
    edit.move_chars(3, true);
    assert_eq!(edit.selected_text(), "Aя🦀");
    edit.move_chars(-2, false);
    assert_eq!(edit.caret(), 0);
    edit.move_chars(isize::MAX, false);
    assert_eq!(edit.caret(), edit.value().len());
    edit.move_chars(isize::MIN, true);
    assert_eq!(edit.selected_text(), "Aя🦀中");
}

#[test]
fn undo_restores_text_and_selection_and_new_edits_drop_redo() {
    let mut edit = TextEdit::new("one".into()).unwrap();
    edit.select_all();
    edit.insert("два").unwrap();
    assert!(edit.undo());
    assert_eq!(edit.value(), "one");
    assert_eq!(edit.selected_text(), "one");
    assert!(edit.redo());
    assert_eq!(edit.value(), "два");
    assert_eq!(edit.caret(), 6);
    edit.undo();
    edit.insert("three").unwrap();
    assert!(!edit.can_redo());
    edit.home(false);
    edit.end(false);
    edit.undo();
    assert_eq!(edit.value(), "one");
    assert!(edit.reset("fresh".into()).unwrap());
    assert!(!edit.can_undo());
    assert!(!edit.can_redo());
}

#[test]
fn edit_history_is_bounded_and_noop_edits_do_not_add_undo() {
    let mut edit = TextEdit::new(String::new()).unwrap();
    assert!(!edit.insert("").unwrap());
    assert!(!edit.can_undo());
    for _ in 0..110 {
        edit.insert("x").unwrap();
    }
    let mut steps = 0;
    while edit.undo() {
        steps += 1;
    }
    assert_eq!(steps, 100);
    assert_eq!(edit.value(), "xxxxxxxxxx");
    assert!(TextEdit::new("one\rtwo".into()).is_err());
}

#[test]
fn data_table_and_virtual_list_models_sort_filter_resize_and_virtualize_rows() {
    let cols = vec![
        TableColumn {
            id: "name".into(),
            width: 120.,
            min_width: 60.,
            max_width: 240.,
        },
        TableColumn {
            id: "score".into(),
            width: 80.,
            min_width: 40.,
            max_width: 160.,
        },
    ];
    let rows = vec![
        vec!["Beta".into(), "20".into()],
        vec!["Alpha".into(), "100".into()],
        vec!["Gamma".into(), "5".into()],
        vec!["Delta".into(), "40".into()],
    ];
    let mut table = DataTableModel::new(cols, rows, 24., 48.).unwrap();
    assert_eq!(table.visible_rows(), &[0, 1, 2, 3]);
    assert_eq!(table.virtual_window(0), 0..2);
    assert_eq!(table.toggle_sort(1).unwrap(), SortDirection::Ascending);
    assert_eq!(table.visible_rows(), &[2, 0, 3, 1]);
    assert_eq!(table.toggle_sort(1).unwrap(), SortDirection::Descending);
    assert_eq!(table.visible_rows(), &[1, 3, 0, 2]);
    assert_eq!(table.resize_column(0, 500.).unwrap(), 240.);
    assert_eq!(table.resize_column(0, 20.).unwrap(), 60.);
    table.set_filter("a".into());
    assert_eq!(table.visible_rows(), &[1, 3, 0, 2]);
    table.set_filter("gam".into());
    assert_eq!(table.visible_rows(), &[2]);

    let mut vlist = VirtualListModel::new(100, 20., 60., 1).unwrap();
    assert_eq!(vlist.visible_range(), 0..4);
    assert_eq!(vlist.select(10).unwrap(), 10);
    assert!(vlist.scroll_y() >= 160.);
    assert!(vlist.visible_range().contains(&10));
}

#[test]
fn rich_text_code_editor_datetime_and_command_palette_models_preserve_invariants() {
    let rich = RichTextModel::from_markdown("## **Forma** *UI* `code`").unwrap();
    assert_eq!(rich.heading_level(), 2);
    assert_eq!(rich.text(), "Forma UI code");
    assert_eq!(rich.spans().len(), 3);
    assert_eq!(rich.to_markdown(), "## Forma UI code");

    let mut code = CodeEditorModel::new("component Demo {\n}".into(), 2).unwrap();
    assert_eq!(code.line_count(), 2);
    assert_eq!(code.cursor_line_column(), (2, 2));
    let tokens = code.syntax_tokens().unwrap();
    assert!(tokens.iter().any(|t| t.value == "component"));
    assert!(code.indent());

    let mut dt = DateTimeModel::new(2024, 2, 28, 14, 30).unwrap();
    assert_eq!(dt.format_iso(), "2024-02-28 14:30");
    dt.step_days(1);
    assert_eq!(dt.format_date(), "2024-02-29");
    dt.step_days(1);
    assert_eq!(dt.format_date(), "2024-03-01");
    assert!(dt.calendar_grid().contains(&31));

    let mut palette = CommandPaletteModel::new(vec![
        CommandItem {
            id: "file.save".into(),
            title: "Save Document".into(),
            category: "File".into(),
            shortcut: "Cmd+S".into(),
        },
        CommandItem {
            id: "view.zoom".into(),
            title: "Zoom In".into(),
            category: "View".into(),
            shortcut: "Cmd++".into(),
        },
    ]);
    palette.set_query("zoom".into());
    assert_eq!(palette.active_command().unwrap().id, "view.zoom");
}
