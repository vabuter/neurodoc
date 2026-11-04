//! Короткая всплывающая подсказка у кнопки (аналог TeachingTip в Windows-версии):
//! «Ссылка скопирована», «Связано», отказы вроде «Курсор внутри ссылки».
//! Гаснет сама через 1,4 с и не забирает фокус у редактора.

use std::rc::Rc;

use gtk::prelude::*;

use super::Win;

impl Win {
    /// `anchor` — ключ кнопки панели инструментов (`new_link`, `copy_link`, …), `git`
    /// или `reload`; неизвестный ключ показывает подсказку у кнопки проектов.
    pub fn show_tip(self: &Rc<Self>, anchor: &str, title: &str, subtitle: &str) {
        let target: gtk::Widget = match anchor {
            "git" => self.git_button.clone().upcast(),
            key => self.toolbar_buttons.get(key).or_else(|| self.toolbar_buttons.get("projects")).cloned().unwrap(),
        };
        self.tip_title.set_text(title);
        self.tip_subtitle.set_text(subtitle);
        self.tip_subtitle.set_visible(!subtitle.is_empty());
        if self.tip.parent().as_ref() != Some(&target) {
            self.tip.unparent();
            self.tip.set_parent(&target);
        }
        self.tip.popup();
        let w = self.clone();
        self.tip_timer.restart(move || {
            w.tip_timer.forget();
            w.tip.popdown();
        });
    }
}
