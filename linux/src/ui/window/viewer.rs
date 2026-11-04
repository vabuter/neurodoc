//! Просмотр содержимого в формате (WebKitGTK) и дельт git.
//!
//! Страницы строит `crate::viewer::html` — те же, что на Windows. Мост к хосту:
//! пользовательский скрипт подставляет `window.chrome.webview.postMessage`, так что
//! общие скрипты страниц работают без изменений, а сообщения приходят в обработчик
//! `script-message-received::neurodoc`.

use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use webkit::prelude::*;

use super::Win;
use crate::domain::catalog;
use crate::ui::css;
use crate::viewer::ViewMode;
use crate::viewer::html::{SVG_BACKGROUND_MESSAGE, build_diff_html, build_viewer_html, svg_background_dark};

impl Win {
    pub(super) fn on_format_toggle(self: &Rc<Self>, mode: ViewMode, active: bool) {
        if self.suppress_toggles.get() {
            return;
        }
        // снятая кнопка означает возврат к тексту
        self.set_view_mode(if active { mode } else { ViewMode::Text });
    }

    fn sync_format_toggles(&self) {
        let mode = self.state.borrow().view_mode;
        self.suppress_toggles.set(true);
        for (t, m) in &self.format_toggles {
            t.set_active(*m == mode);
        }
        self.suppress_toggles.set(false);
    }

    /// Возврат в текстовый режим (при открытии другого файла и т.п.).
    pub fn reset_view_mode(&self) {
        let mut st = self.state.borrow_mut();
        // дельта показывается при текстовом режиме, ранний выход её бы не снял
        if st.diff_active {
            st.diff_active = false;
            st.diff_key = None;
            drop(st);
            self.editor_stack.set_visible_child_name("editor");
            st = self.state.borrow_mut();
        }
        if st.view_mode == ViewMode::Text {
            return;
        }
        st.view_mode = ViewMode::Text;
        drop(st);
        self.sync_format_toggles();
        self.editor_stack.set_visible_child_name("editor");
    }

    pub fn set_view_mode(self: &Rc<Self>, mode: ViewMode) {
        {
            let mut st = self.state.borrow_mut();
            st.diff_active = false;
            st.diff_key = None;
        }
        // «Как есть» — такой же режим показа, как остальные: включая формат, его снимаем
        if mode != ViewMode::Text && self.state.borrow().settings.raw_editing {
            self.state.borrow_mut().settings.raw_editing = false;
            self.save_settings();
            self.sync_raw_toggle();
        }
        self.state.borrow_mut().view_mode = mode;
        self.sync_format_toggles();
        if mode == ViewMode::Text {
            self.editor_stack.set_visible_child_name("editor");
            self.editor.grab_focus();
            return;
        }
        self.save_current_file();
        let html = {
            let st = self.state.borrow();
            let dark = self.is_dark();
            build_viewer_html(mode, &self.editor_text_for_save(), dark, svg_background_dark(st.settings.current_project(), dark), &st.settings)
        };
        let web = self.ensure_webview();
        web.set_background_color(&css::rgba(if self.is_dark() { css::EDITOR_BG_DARK } else { css::EDITOR_BG_LIGHT }, 1.0));
        web.load_html(&html, None);
        self.editor_stack.set_visible_child_name("viewer");
    }

    /// Перерисовать просмотр (смена темы, шрифта), если он активен.
    pub fn refresh_viewer_if_active(&self) {
        let (mode, diff) = {
            let st = self.state.borrow();
            (st.view_mode, st.diff_active)
        };
        if diff || mode == ViewMode::Text {
            return;
        }
        let html = {
            let st = self.state.borrow();
            let dark = self.is_dark();
            build_viewer_html(mode, &self.editor_text_for_save(), dark, svg_background_dark(st.settings.current_project(), dark), &st.settings)
        };
        if let Some(web) = self.webview.borrow().as_ref() {
            web.set_background_color(&css::rgba(if self.is_dark() { css::EDITOR_BG_DARK } else { css::EDITOR_BG_LIGHT }, 1.0));
            web.load_html(&html, None);
        }
    }

    /// Самопроверка (NEURODOC_SMOKE): выполнить скрипт в просмотре, результат — строкой.
    pub async fn smoke_eval(&self, script: &str) -> Option<String> {
        let web = self.webview.borrow().clone()?;
        let value = web.evaluate_javascript_future(script, None, None).await.ok()?;
        Some(value.to_str().to_string())
    }

    /// Готовит WebView — один раз на всё приложение, при первом обращении.
    fn ensure_webview(self: &Rc<Self>) -> webkit::WebView {
        if let Some(w) = self.webview.borrow().as_ref() {
            return w.clone();
        }
        let web = webkit::WebView::new();
        web.set_vexpand(true);
        web.set_hexpand(true);
        if let Some(settings) = WebViewExt::settings(&web) {
            settings.set_enable_developer_extras(false);
            settings.set_allow_file_access_from_file_urls(false);
            settings.set_allow_universal_access_from_file_urls(false);
            settings.set_javascript_can_open_windows_automatically(false);
            settings.set_enable_back_forward_navigation_gestures(false);
            settings.set_enable_html5_local_storage(false);
            settings.set_enable_html5_database(false);
            settings.set_enable_page_cache(false);
        }
        if let Some(ucm) = web.user_content_manager() {
            ucm.register_script_message_handler("neurodoc", None);
            let w = self.clone();
            ucm.connect_script_message_received(Some("neurodoc"), move |_, value| {
                let message = value.to_str().to_string();
                let w = w.clone();
                glib::idle_add_local_once(move || w.on_viewer_message(&message));
            });
            let shim = webkit::UserScript::new(
                "window.chrome={webview:{postMessage:function(m){window.webkit.messageHandlers.neurodoc.postMessage(String(m));}}};",
                webkit::UserContentInjectedFrames::AllFrames,
                webkit::UserScriptInjectionTime::Start,
                &[],
                &[],
            );
            ucm.add_script(&shim);
        }
        // ссылку не даём открыть самому WebView — он ушёл бы с отрисованной страницы
        web.connect_decide_policy(|_, decision, kind| {
            if kind == webkit::PolicyDecisionType::NavigationAction || kind == webkit::PolicyDecisionType::NewWindowAction {
                let uri = decision
                    .clone()
                    .downcast::<webkit::NavigationPolicyDecision>()
                    .ok()
                    .and_then(|n| n.navigation_action())
                    .and_then(|a| a.request())
                    .and_then(|r| r.uri())
                    .map(|u| u.to_string())
                    .unwrap_or_default();
                if !uri.is_empty() && uri != "about:blank" {
                    decision.ignore();
                    return true;
                }
            }
            false
        });
        web.connect_context_menu(|_, _, _| true);
        self.viewer_box.append(&web);
        *self.webview.borrow_mut() = Some(web.clone());
        web
    }

    /// Клик по ссылке в отрендеренном документе (или смена фона SVG).
    fn on_viewer_message(self: &Rc<Self>, target: &str) {
        if target.is_empty() {
            return;
        }
        if let Some(rest) = target.strip_prefix(SVG_BACKGROUND_MESSAGE) {
            self.save_svg_background(rest == "dark");
            return;
        }
        if catalog::is_web_target(target) {
            self.launch_web_link(target);
            return;
        }
        // несуществующая цель — не повод никуда уходить: в том же ReadMe.md ссылки
        // из описания формата ведут на файлы-примеры, которых нет
        let resolved = {
            let st = self.state.borrow();
            match (&st.catalog, &st.current_id) {
                (Some(c), Some(cur)) => catalog::try_resolve_link(cur, target).filter(|id| c.exists(id)),
                _ => None,
            }
        };
        if let Some(id) = resolved {
            self.open_file(&id, true);
        }
    }

    /// Запоминает выбранный в просмотре фон за текущим проектом.
    fn save_svg_background(&self, dark: bool) {
        let changed = {
            let mut st = self.state.borrow_mut();
            let value = if dark { "Dark" } else { "Light" };
            match st.settings.current_project_mut() {
                Some(p) if p.svg_background.as_deref() != Some(value) => {
                    p.svg_background = Some(value.to_string());
                    true
                }
                _ => false,
            }
        };
        if changed {
            self.save_settings();
        }
    }

    // ================= Дельта =================

    /// Показывает дельту файла в просмотрщике. Повторный вызов с тем же ключом закрывает —
    /// кнопка «±» работает как переключатель.
    pub fn show_diff(self: &Rc<Self>, key: &str, title: &str, unified_diff: &str) {
        let same = {
            let st = self.state.borrow();
            st.diff_active && st.diff_key.as_deref() == Some(key)
        };
        if same {
            self.reset_view_mode();
            return;
        }
        let html = {
            let st = self.state.borrow();
            build_diff_html(title, unified_diff, self.is_dark(), &st.settings)
        };
        let web = self.ensure_webview();
        web.set_background_color(&css::rgba(if self.is_dark() { css::EDITOR_BG_DARK } else { css::EDITOR_BG_LIGHT }, 1.0));
        web.load_html(&html, None);
        self.editor_stack.set_visible_child_name("viewer");
        let mut st = self.state.borrow_mut();
        st.diff_active = true;
        st.diff_key = Some(key.to_string());
    }
}
