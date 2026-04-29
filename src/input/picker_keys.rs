pub(crate) enum PickerAction {
    Hide,
    Confirm,
    MoveDown,
    MoveUp,
    Unhandled,
}

pub(crate) fn resolve_picker_key(key_name: &str, is_ctrl: bool) -> PickerAction {
    if is_ctrl {
        match key_name {
            "n" => return PickerAction::MoveDown,
            "p" => return PickerAction::MoveUp,
            _ => {}
        }
    }
    match key_name {
        "Escape" => PickerAction::Hide,
        "Return" => PickerAction::Confirm,
        "Down" => PickerAction::MoveDown,
        "Up" => PickerAction::MoveUp,
        _ => PickerAction::Unhandled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_returns_hide() {
        assert!(matches!(resolve_picker_key("Escape", false), PickerAction::Hide));
    }

    #[test]
    fn return_returns_confirm() {
        assert!(matches!(resolve_picker_key("Return", false), PickerAction::Confirm));
    }

    #[test]
    fn down_and_ctrl_n_return_move_down() {
        assert!(matches!(resolve_picker_key("Down", false), PickerAction::MoveDown));
        assert!(matches!(resolve_picker_key("n", true), PickerAction::MoveDown));
    }

    #[test]
    fn up_and_ctrl_p_return_move_up() {
        assert!(matches!(resolve_picker_key("Up", false), PickerAction::MoveUp));
        assert!(matches!(resolve_picker_key("p", true), PickerAction::MoveUp));
    }

    #[test]
    fn other_keys_return_unhandled() {
        assert!(matches!(resolve_picker_key("j", false), PickerAction::Unhandled));
        assert!(matches!(resolve_picker_key("k", false), PickerAction::Unhandled));
        assert!(matches!(resolve_picker_key("a", false), PickerAction::Unhandled));
        assert!(matches!(resolve_picker_key("n", false), PickerAction::Unhandled));
    }

    #[test]
    fn ctrl_with_non_nav_keys_returns_unhandled() {
        assert!(matches!(resolve_picker_key("a", true), PickerAction::Unhandled));
        assert!(matches!(resolve_picker_key("j", true), PickerAction::Unhandled));
    }
}
