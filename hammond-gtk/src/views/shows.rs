use failure::Error;
use gtk;
use gtk::prelude::*;

use hammond_data::Podcast;
use hammond_data::dbqueries;

use app::Action;
use utils::{get_ignored_shows, get_pixbuf_from_path};

use std::sync::Arc;
use std::sync::mpsc::Sender;

#[derive(Debug, Clone)]
pub struct ShowsPopulated {
    pub container: gtk::Box,
    scrolled_window: gtk::ScrolledWindow,
    flowbox: gtk::FlowBox,
}

impl Default for ShowsPopulated {
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
    pub fn new(sender: Sender<Action>) -> Result<ShowsPopulated, Error> {
        let pop = ShowsPopulated::default();
        pop.init(sender)?;
        Ok(pop)
    }

    pub fn init(&self, sender: Sender<Action>) -> Result<(), Error> {
        self.flowbox.connect_child_activated(move |_, child| {
            if let Err(err) = on_child_activate(child, sender.clone()) {
                error!(
                    "Something went wrong during flowbox child activation: {}.",
                    err
                )
            };
        });
        // Populate the flowbox with the Podcasts.
        self.populate_flowbox()
    }

    fn populate_flowbox(&self) -> Result<(), Error> {
        let ignore = get_ignored_shows()?;
        let podcasts = dbqueries::get_podcasts_filter(&ignore)?;

        podcasts.into_iter().map(Arc::new).for_each(|parent| {
            let flowbox_child = ShowsChild::new(parent);
            self.flowbox.add(&flowbox_child.child);
        });
        self.flowbox.show_all();
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.flowbox.get_children().is_empty()
    }

    /// Set scrolled window vertical adjustment.
    pub fn set_vadjustment(&self, vadjustment: &gtk::Adjustment) {
        self.scrolled_window.set_vadjustment(vadjustment)
    }
}

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
    pub fn new(pd: Arc<Podcast>) -> ShowsChild {
        let child = ShowsChild::default();
        child.init(pd);
        child
    }

    fn init(&self, pd: Arc<Podcast>) {
        self.container.set_tooltip_text(pd.title());

        if let Err(err) = self.set_cover(pd.clone()) {
            error!("Failed to set a cover: {}", err)
        }

        WidgetExt::set_name(&self.child, &pd.id().to_string());
    }

    fn set_cover(&self, pd: Arc<Podcast>) -> Result<(), Error> {
        let image = get_pixbuf_from_path(&pd.clone().into(), 256)?;
        self.cover.set_from_pixbuf(&image);
        Ok(())
    }
}
