use glib;
use gtk;
use gtk::prelude::*;

use failure::Error;
use html2pango::markup_from_raw;
use open;
use rayon;

use hammond_data::dbqueries;
use hammond_data::utils::delete_show;
use hammond_data::Podcast;

use app::Action;
use appnotif::InAppNotification;
use utils::{self, lazy_load};
use widgets::EpisodeWidget;

use std::sync::mpsc::{SendError, Sender};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ShowWidget {
    pub container: gtk::Box,
    scrolled_window: gtk::ScrolledWindow,
    cover: gtk::Image,
    description: gtk::Label,
    link: gtk::Button,
    settings: gtk::MenuButton,
    unsub: gtk::Button,
    episodes: gtk::ListBox,
    progress_bar: gtk::ProgressBar,
}

impl Default for ShowWidget {
    fn default() -> Self {
        let builder = gtk::Builder::new_from_resource("/org/gnome/hammond/gtk/show_widget.ui");
        let container: gtk::Box = builder.get_object("container").unwrap();
        let scrolled_window: gtk::ScrolledWindow = builder.get_object("scrolled_window").unwrap();
        let episodes = builder.get_object("episodes").unwrap();

        let cover: gtk::Image = builder.get_object("cover").unwrap();
        let description: gtk::Label = builder.get_object("description").unwrap();
        let unsub: gtk::Button = builder.get_object("unsub_button").unwrap();
        let link: gtk::Button = builder.get_object("link_button").unwrap();
        let settings: gtk::MenuButton = builder.get_object("settings_button").unwrap();
        let progress_bar = builder.get_object("progress_bar").unwrap();

        ShowWidget {
            container,
            scrolled_window,
            cover,
            description,
            unsub,
            link,
            settings,
            episodes,
            progress_bar,
        }
    }
}

impl ShowWidget {
    pub fn new(pd: Arc<Podcast>, sender: Sender<Action>) -> ShowWidget {
        let pdw = ShowWidget::default();
        pdw.init(pd, sender);
        pdw
    }

    pub fn init(&self, pd: Arc<Podcast>, sender: Sender<Action>) {
        let builder = gtk::Builder::new_from_resource("/org/gnome/hammond/gtk/show_widget.ui");

        // Hacky workaround so the pd.id() can be retrieved from the `ShowStack`.
        WidgetExt::set_name(&self.container, &pd.id().to_string());

        self.unsub
            .connect_clicked(clone!(pd, sender => move |bttn| {
                on_unsub_button_clicked(pd.clone(), bttn, sender.clone());
        }));

        self.set_description(pd.description());

        self.populate_listbox(pd.clone(), sender.clone())
            .map_err(|err| error!("Failed to populate the listbox: {}", err))
            .ok();

        self.set_cover(pd.clone())
            .map_err(|err| error!("Failed to set a cover: {}", err))
            .ok();

        let link = pd.link().to_owned();
        self.link.set_tooltip_text(Some(link.as_str()));
        self.link.connect_clicked(move |_| {
            info!("Opening link: {}", &link);
            open::that(&link)
                .map_err(|err| error!("Error: {}", err))
                .map_err(|_| error!("Failed open link: {}", &link))
                .ok();
        });

        let show_menu: gtk::Popover = builder.get_object("show_menu").unwrap();
        let mark_all: gtk::ModelButton = builder.get_object("mark_all_watched").unwrap();

        let episodes = self.episodes.clone();
        mark_all.connect_clicked(clone!(pd, sender => move |_| {
            on_played_button_clicked(
                pd.clone(),
                &episodes,
                sender.clone()
            )
        }));
        self.settings.set_popover(&show_menu);
    }

    /// Set the show cover.
    fn set_cover(&self, pd: Arc<Podcast>) -> Result<(), Error> {
        utils::set_image_from_path(&self.cover, Arc::new(pd.into()), 128)
    }

    /// Set the descripton text.
    fn set_description(&self, text: &str) {
        self.description.set_markup(&markup_from_raw(text));
    }

    /// Set scrolled window vertical adjustment.
    pub fn set_vadjustment(&self, vadjustment: &gtk::Adjustment) {
        self.scrolled_window.set_vadjustment(vadjustment)
    }

    /// Populate the listbox with the shows episodes.
    fn populate_listbox(&self, pd: Arc<Podcast>, sender: Sender<Action>) -> Result<(), Error> {
        use crossbeam_channel::bounded;
        use crossbeam_channel::TryRecvError::*;

        let count = dbqueries::get_pd_episodes_count(&pd)?;

        let (sender_, receiver) = bounded(1);
        rayon::spawn(clone!(pd => move || {
            let episodes = dbqueries::get_pd_episodeswidgets(&pd).unwrap();
            // The receiver can be dropped if there's an early return
            // like on show without episodes for example.
            sender_.send(episodes).ok();
        }));

        if count == 0 {
            let builder = gtk::Builder::new_from_resource("/org/gnome/hammond/gtk/empty_show.ui");
            let container: gtk::Box = builder.get_object("empty_show").unwrap();
            self.episodes.add(&container);
            return Ok(());
        }

        let list = self.episodes.clone();
        let bar = self.progress_bar.clone();
        gtk::idle_add(move || {
            let episodes = match receiver.try_recv() {
                Ok(e) => e,
                Err(Empty) => return glib::Continue(true),
                Err(Disconnected) => return glib::Continue(false),
            };

            let mut done = 0;
            let constructor = clone!(sender, bar => move |ep| {
                done += 1;
                if done >= count {
                    bar.hide()
                }

                let fraction = done as f64 / count as f64;
                bar.set_fraction(fraction);

                EpisodeWidget::new(ep, sender.clone()).container
            });

            let callback = clone!(pd, sender => move || {
                sender.send(Action::SetShowWidgetAlignment(pd.clone()))
                    .map_err(|err| error!("Action Sender: {}", err))
                    .ok();
            });

            bar.show();
            lazy_load(episodes, list.clone(), constructor, callback);

            glib::Continue(false)
        });

        Ok(())
    }
}

fn on_unsub_button_clicked(pd: Arc<Podcast>, unsub_button: &gtk::Button, sender: Sender<Action>) {
    // hack to get away without properly checking for none.
    // if pressed twice would panic.
    unsub_button.set_sensitive(false);

    let wrap = || -> Result<(), SendError<_>> {
        sender.send(Action::RemoveShow(pd))?;

        sender.send(Action::HeaderBarNormal)?;
        sender.send(Action::ShowShowsAnimated)?;
        // Queue a refresh after the switch to avoid blocking the db.
        sender.send(Action::RefreshShowsView)?;
        sender.send(Action::RefreshEpisodesView)?;
        Ok(())
    };

    wrap().map_err(|err| error!("Action Sender: {}", err)).ok();
    unsub_button.set_sensitive(true);
}

fn on_played_button_clicked(pd: Arc<Podcast>, episodes: &gtk::ListBox, sender: Sender<Action>) {
    if dim_titles(episodes).is_none() {
        error!("Something went horribly wrong when dimming the titles.");
        warn!("RUN WHILE YOU STILL CAN!");
    }

    sender
        .send(Action::MarkAllPlayerNotification(pd))
        .map_err(|err| error!("Action Sender: {}", err))
        .ok();
}

fn mark_all_watched(pd: &Podcast, sender: Sender<Action>) -> Result<(), Error> {
    dbqueries::update_none_to_played_now(pd)?;
    // Not all widgets migth have been loaded when the mark_all is hit
    // So we will need to refresh again after it's done.
    sender.send(Action::RefreshWidgetIfSame(pd.id()))?;
    sender.send(Action::RefreshEpisodesView).map_err(From::from)
}

pub fn mark_all_notif(pd: Arc<Podcast>, sender: Sender<Action>) -> InAppNotification {
    let id = pd.id();
    let callback = clone!(sender => move || {
        mark_all_watched(&pd, sender.clone())
            .map_err(|err| error!("Notif Callback Error: {}", err))
            .ok();
        glib::Continue(false)
    });

    let undo_callback = clone!(sender => move || {
        sender.send(Action::RefreshWidgetIfSame(id))
            .map_err(|err| error!("Action Sender: {}", err))
            .ok();
    });

    let text = "Marked all episodes as listened".into();
    InAppNotification::new(text, callback, undo_callback)
}

pub fn remove_show_notif(pd: Arc<Podcast>, sender: Sender<Action>) -> InAppNotification {
    let text = format!("Unsubscribed from {}", pd.title());

    utils::ignore_show(pd.id())
        .map_err(|err| error!("Error: {}", err))
        .map_err(|_| error!("Could not insert {} to the ignore list.", pd.title()))
        .ok();

    let callback = clone!(pd => move || {
        utils::uningore_show(pd.id())
            .map_err(|err| error!("Error: {}", err))
            .map_err(|_| error!("Could not remove {} from the ignore list.", pd.title()))
            .ok();

        // Spawn a thread so it won't block the ui.
        rayon::spawn(clone!(pd => move || {
            delete_show(&pd)
                .map_err(|err| error!("Error: {}", err))
                .map_err(|_| error!("Failed to delete {}", pd.title()))
                .ok();
        }));
        glib::Continue(false)
    });

    let undo_wrap = move || -> Result<(), Error> {
        utils::uningore_show(pd.id())?;
        sender.send(Action::RefreshShowsView)?;
        sender.send(Action::RefreshEpisodesView)?;
        Ok(())
    };

    let undo_callback = move || {
        undo_wrap().map_err(|err| error!("{}", err)).ok();
    };

    InAppNotification::new(text, callback, undo_callback)
}

// Ideally if we had a custom widget this would have been as simple as:
// `for row in listbox { ep = row.get_episode(); ep.dim_title(); }`
// But now I can't think of a better way to do it than hardcoding the title
// position relative to the EpisodeWidget container gtk::Box.
fn dim_titles(episodes: &gtk::ListBox) -> Option<()> {
    let children = episodes.get_children();

    for row in children {
        let row = row.downcast::<gtk::ListBoxRow>().ok()?;
        let container = row.get_children().remove(0).downcast::<gtk::Box>().ok()?;
        let foo = container
            .get_children()
            .remove(0)
            .downcast::<gtk::Box>()
            .ok()?;
        let bar = foo.get_children().remove(0).downcast::<gtk::Box>().ok()?;
        let baz = bar.get_children().remove(0).downcast::<gtk::Box>().ok()?;
        let title = baz.get_children().remove(0).downcast::<gtk::Label>().ok()?;

        title.get_style_context().map(|c| c.add_class("dim-label"));
    }
    Some(())
}
