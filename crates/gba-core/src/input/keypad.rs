//! Functional keypad IRQ sampling from input snapshots and control writes.
//! OR requests on every matching sample. AND suppresses identical matching
//! snapshots, following mGBA's polling model. Exact hardware edges are unverified.

use super::Buttons;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Keypad {
    buttons: Buttons,
    control: u16,
    last_match: Option<u16>,
}

impl Keypad {
    pub(crate) fn keyinput(&self) -> u16 {
        self.buttons.keyinput()
    }

    pub(crate) fn control(&self) -> u16 {
        self.control
    }

    pub(crate) fn set_buttons(&mut self, buttons: Buttons) -> bool {
        self.buttons = buttons;
        self.sample()
    }

    pub(crate) fn write_control(&mut self, value: u16) -> bool {
        let value = value & 0xc3ff;
        let newly_selected = (value & !self.control) & 0x03ff;
        // A newly selected held key must not be hidden by an earlier AND sample.
        if let Some(previous) = &mut self.last_match {
            *previous &= !newly_selected;
        }
        self.control = value;
        self.sample()
    }

    fn sample(&mut self) -> bool {
        if self.control & 0x4000 == 0 {
            return false;
        }
        let selected = self.control & 0x03ff;
        let pressed = self.buttons.bits();
        let all = self.control & 0x8000 != 0;
        let matched = if all {
            pressed & selected == selected
        } else {
            pressed & selected != 0
        };
        if !matched {
            self.last_match = None;
            return false;
        }
        let previous = self.last_match.replace(pressed);
        !all || previous != Some(pressed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_masks_and_button_states_match_independent_per_button_logic() {
        for selected in 0..1024 {
            for pressed in 0..1024 {
                let mut any = false;
                let mut all = true;
                for bit in 0..10 {
                    if selected & (1 << bit) != 0 {
                        any |= pressed & (1 << bit) != 0;
                        all &= pressed & (1 << bit) != 0;
                    }
                }
                for (condition, expected) in [(0, any), (0x8000, all)] {
                    let mut keypad = Keypad::default();
                    assert!(!keypad.set_buttons(Buttons::from_bits(pressed)));
                    assert_eq!(
                        keypad.write_control(selected | condition | 0x4000),
                        expected
                    );
                    assert_eq!(keypad.keyinput(), !pressed & 0x03ff);
                }
            }
        }
    }
}
