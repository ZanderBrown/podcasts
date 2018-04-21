use failure::Error;
use gtk;
use gtk::prelude::*;

use hammond_data::dbqueries;
use hammond_data::Podcast;

use app::Action;
use utils::{get_ignored_shows, lazy_load, set_image_from_path};

use std::rc::Rc;
use std::sync::mpsc::Sender;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ShowsPopulated {
    pub container: gtk::Box,
    scrolled_window: gtk::ScrolledWindow,
    flowbox: gtk::FlowBox,
}

impl Default for ShowsPopulated {
    #[inline]
    fn default() -> Self {
        let builder = gtk::Builder::new_from_resource("/org/gnome/hammond/gtk/shows_view.ui");
        let container: gtk::Box = builder.get_object("fb_parent").unwrap();
        let scrolled_window: gtk::ScrolledWindow = builder.get_object("scrolled_window").unwrap();
        let flowbox: gtk::FlowBox = builder.get_object("flowbox").unwrap();

        ShowsPopulated {
            container,
            scrolled_window,
            flowbox,
        }
    }
}

impl ShowsPopulated {
    #[inline]
    pub fn new(sender: Sender<Action>) -> Result<Rc<ShowsPopulated>, Error> {
        let pop = Rc::new(ShowsPopulated::default());
        pop.init(sender)?;
        Ok(pop)
    }

    #[inline]
    pub fn init(&self, sender: Sender<Action>) -> Result<(), Error> {
        self.flowbox.connect_child_activated(move |_, child| {
            on_child_activate(child, sender.clone())
                .map_err(|err| error!("Error along flowbox child activation: {}", err))
                .ok();
        });
        // Populate the flowbox with the Podcasts.
        self.populate_flowbox()
    }

    #[inline]
    fn populate_flowbox(&self) -> Result<(), Error> {
        let ignore = get_ignored_shows()?;
        let podcasts = dbqueries::get_podcasts_filter(&ignore)?;

        let flowbox = self.flowbox.clone();
        let constructor = |parent| ShowsChild::new(&parent).child;
        let callback = move || flowbox.show_all();

        let flowbox = self.flowbox.clone();
        lazy_load(podcasts, flowbox, constructor, callback);

        Ok(())
    }
}

#[inline]
fn on_child_activate(child: &gtk::FlowBoxChild, sender: Sender<Action>) -> Result<(), Error> {
    use gtk::WidgetExt;

    // This is such an ugly hack...
    let id = WidgetExt::get_name(child)
        .ok_or_else(|| format_err!("Faild to get \"episodes\" child from the stack."))?
        .parse::<i32>()?;
    let pd = Arc::new(dbqueries::get_podcast_from_id(id)?);

    sender.send(Action::HeaderBarShowTile(pd.title().into()))?;
    sender.send(Action::ReplaceWidget(pd))?;
    sender.send(Action::ShowWidgetAnimated)?;
    Ok(())
}

#[derive(Debug)]
struct ShowsChild {
    container: gtk::Box,
    cover: gtk::Image,
    child: gtk::FlowBoxChild,
}

impl Default for ShowsChild {
    #[inline]
    fn default() -> Self {
        let builder = gtk::Builder::new_from_resource("/org/gnome/hammond/gtk/shows_child.ui");

        let container: gtk::Box = builder.get_object("fb_child").unwrap();
        let cover: gtk::Image = builder.get_object("pd_cover").unwrap();

        let child = gtk::FlowBoxChild::new();
        child.add(&container);

        ShowsChild {
            container,
            cover,
            child,
        }
    }
}

impl ShowsChild {
    #[inline]
    pub fn new(pd: &Podcast) -> ShowsChild {
        let child = ShowsChild::default();
        child.init(pd);
        child
    }

    #[inline]
    fn init(&self, pd: &Podcast) {
        self.container.set_tooltip_text(pd.title());
        WidgetExt::set_name(&self.child, &pd.id().to_string());

        self.set_cover(pd.id())
            .map_err(|err| error!("Failed to set a cover: {}", err))
            .ok();
    }

    #[inline]
    fn set_cover(&self, podcast_id: i32) -> Result<(), Error> {
        set_image_from_path(&self.cover, podcast_id, 256)
    }
}
