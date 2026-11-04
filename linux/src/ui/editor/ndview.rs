//! Наследник GtkTextView, который после собственной отрисовки рисует «кружки»
//! ссылок с пустым якорем: показывать там нечего, и ссылка целиком уходит под
//! непрозрачный кружок (как ChipKind.Solid на Windows). Рисование в snapshot
//! идёт в координатах виджета и само едет вместе с прокруткой.

use std::cell::RefCell;

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

pub type DrawExtra = Box<dyn Fn(&gtk::TextView, &gtk::Snapshot)>;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct NdTextView {
        pub draw_extra: RefCell<Option<DrawExtra>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for NdTextView {
        const NAME: &'static str = "NdTextView";
        type Type = super::NdTextView;
        type ParentType = gtk::TextView;
    }

    impl ObjectImpl for NdTextView {}

    impl WidgetImpl for NdTextView {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            self.parent_snapshot(snapshot);
            if let Some(draw) = &*self.draw_extra.borrow() {
                draw(self.obj().upcast_ref(), snapshot);
            }
        }
    }

    impl TextViewImpl for NdTextView {}
}

glib::wrapper! {
    pub struct NdTextView(ObjectSubclass<imp::NdTextView>)
        @extends gtk::TextView, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Scrollable;
}

impl Default for NdTextView {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl NdTextView {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_draw_extra(&self, draw: DrawExtra) {
        *self.imp().draw_extra.borrow_mut() = Some(draw);
    }
}
