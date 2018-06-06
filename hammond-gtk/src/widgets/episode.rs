use glib;
use gtk;
use gtk::prelude::*;

use chrono;
use chrono::prelude::*;
use crossbeam_channel::Sender;
use failure::Error;
use humansize::{file_size_opts as size_opts, FileSize};
use open;

use hammond_data::dbqueries;
use hammond_data::utils::get_download_folder;
use hammond_data::EpisodeWidgetQuery;

use app::Action;
use manager;

use std::path::Path;
use std::rc::Rc;
use std::sync::{Arc, Mutex, TryLockError};

lazy_static! {
    static ref SIZE_OPTS: Arc<size_opts::FileSizeOpts> =  {
        // Declare a custom humansize option struct
        // See: https://docs.rs/humansize/1.0.2/humansize/file_size_opts/struct.FileSizeOpts.html
        Arc::new(size_opts::FileSizeOpts {
            divider: size_opts::Kilo::Binary,
            units: size_opts::Kilo::Decimal,
            decimal_places: 0,
            decimal_zeroes: 0,
            fixed_at: size_opts::FixedAt::No,
            long_units: false,
            space: true,
            suffix: "",
            allow_negative: false,
        })
    };
}

#[derive(Clone, Debug)]
pub struct EpisodeWidget {
    pub container: gtk::Box,
    info: InfoLabels,
    buttons: Buttons,
    progressbar: gtk::ProgressBar,
}

#[derive(Clone, Debug)]
struct InfoLabels {
    container: gtk::Box,
    title: gtk::Label,
    date: gtk::Label,
    separator1: gtk::Label,
    duration: gtk::Label,
    separator2: gtk::Label,
    local_size: gtk::Label,
    size_separator: gtk::Label,
    total_size: gtk::Label,
}

#[derive(Clone, Debug)]
struct Buttons {
    container: gtk::ButtonBox,
    play: gtk::Button,
    download: gtk::Button,
    cancel: gtk::Button,
    desc: gtk::Button,
}

impl InfoLabels {
    fn init(&self, episode: &EpisodeWidgetQuery) {
        // Set the title label state.
        self.set_title(episode);

        // Set the date label.
        self.set_date(episode.epoch());

        // Set the duaration label.
        self.set_duration(episode.duration());

        // Set the total_size label.
        self.set_size(episode.length())
    }

    fn set_title(&self, episode: &EpisodeWidgetQuery) {
        self.title.set_text(episode.title());

        if episode.played().is_some() {
            self.title
                .get_style_context()
                .map(|c| c.add_class("dim-label"));
        } else {
            self.title
                .get_style_context()
                .map(|c| c.remove_class("dim-label"));
        }
    }

    // Set the date label of the episode widget.
    fn set_date(&self, epoch: i32) {
        lazy_static! {
            static ref NOW: DateTime<Utc> = Utc::now();
        };

        let ts = Utc.timestamp(i64::from(epoch), 0);

        // If the episode is from a different year, print year as well
        if NOW.year() != ts.year() {
            self.date.set_text(ts.format("%e %b %Y").to_string().trim());
        // Else omit the year from the label
        } else {
            self.date.set_text(ts.format("%e %b").to_string().trim());
        }
    }

    // Set the duration label of the episode widget.
    fn set_duration(&self, seconds: Option<i32>) {
        // If lenght is provided
        if let Some(s) = seconds {
            // Convert seconds to minutes
            let minutes = chrono::Duration::seconds(s.into()).num_minutes();
            // If the lenght is 1 or more minutes
            if minutes != 0 {
                // Set the label and show them.
                self.duration.set_text(&format!("{} min", minutes));
                self.duration.show();
                self.separator1.show();
                return;
            }
        }

        // Else hide the labels
        self.separator1.hide();
        self.duration.hide();
    }

    // Set the size label of the episode widget.
    fn set_size(&self, bytes: Option<i32>) {
        // Convert the bytes to a String label
        let size = || -> Option<String> {
            let s = bytes?;
            if s == 0 {
                return None;
            }

            s.file_size(SIZE_OPTS.clone()).ok()
        };

        if let Some(s) = size() {
            self.total_size.set_text(&s);
            self.total_size.show();
            self.separator2.show();
        } else {
            self.total_size.hide();
            self.separator2.hide();
        }
    }
}

impl Default for EpisodeWidget {
    fn default() -> Self {
        let builder = gtk::Builder::new_from_resource("/org/gnome/Hammond/gtk/episode_widget.ui");

        let container = builder.get_object("episode_container").unwrap();
        let progressbar = builder.get_object("progress_bar").unwrap();

        let buttons_container = builder.get_object("button_box").unwrap();
        let download = builder.get_object("download_button").unwrap();
        let play = builder.get_object("play_button").unwrap();
        let cancel = builder.get_object("cancel_button").unwrap();
        let desc_button = builder.get_object("desc_button").unwrap();

        let info_container = builder.get_object("info_container").unwrap();
        let title = builder.get_object("title_label").unwrap();
        let date = builder.get_object("date_label").unwrap();
        let duration = builder.get_object("duration_label").unwrap();
        let local_size = builder.get_object("local_size").unwrap();
        let total_size = builder.get_object("total_size").unwrap();

        let separator1 = builder.get_object("separator1").unwrap();
        let separator2 = builder.get_object("separator2").unwrap();

        let size_separator = builder.get_object("prog_separator").unwrap();

        EpisodeWidget {
            info: InfoLabels {
                container: info_container,
                title,
                date,
                separator1,
                duration,
                separator2,
                local_size,
                total_size,
                size_separator,
            },
            buttons: Buttons {
                container: buttons_container,
                play,
                download,
                cancel,
                desc: desc_button,
            },
            progressbar,
            container,
        }
    }
}

impl EpisodeWidget {
    pub fn new(episode: &EpisodeWidgetQuery, sender: &Sender<Action>) -> Rc<Self> {
        let widget = Rc::new(Self::default());
        widget.info.init(episode);
        Self::determine_buttons_state(&widget, episode, sender)
            .map_err(|err| error!("Error: {}", err))
            .ok();
        widget
            .on_desc_clicked(episode, sender)
            .map_err(|err| error!("Error: {}", err))
            .ok();
        widget
    }

    fn on_desc_clicked(
        &self,
        episode: &EpisodeWidgetQuery,
        sender: &Sender<Action>,
    ) -> Result<(), Error> {
        let id = episode.rowid();
        if dbqueries::episode_had_description(id)? {
            self.buttons.desc.show();
            self.buttons
                .desc
                .connect_clicked(clone!(sender => move |_| {
                sender.send(Action::AboutWidget(id))
                    .map_err(|err| error!("Action Sender: {}", err))
                    .ok();
            }));
            Ok(())
        } else {
            self.buttons.desc.hide();
            Ok(())
        }
    }

    // fn init(widget: Rc<Self>, sender: &Sender<Action>) {}

    // InProgress State:
    //   * Show ProgressBar and Cancel Button.
    //   * Show `total_size`, `local_size` labels and `size_separator`.
    //   * Hide Download and Play Buttons
    fn state_prog(&self) {
        self.progressbar.show();
        self.buttons.cancel.show();

        self.info.total_size.show();
        self.info.local_size.show();
        self.info.size_separator.show();

        self.buttons.play.hide();
        self.buttons.download.hide();
    }

    // Playable State:
    //   * Hide ProgressBar and Cancel, Download Buttons.
    //   * Hide `local_size` labels and `size_separator`.
    //   * Show Play Button and `total_size` label
    fn state_playable(&self) {
        self.progressbar.hide();
        self.buttons.cancel.hide();
        self.buttons.download.hide();
        self.info.local_size.hide();
        self.info.size_separator.hide();

        self.info.total_size.show();
        self.buttons.play.show();
    }

    // ToDownload State:
    //   * Hide ProgressBar and Cancel, Play Buttons.
    //   * Hide `local_size` labels and `size_separator`.
    //   * Show Download Button
    //   * Determine `total_size` label state (Comes from `episode.lenght`).
    fn state_download(&self) {
        self.progressbar.hide();
        self.buttons.cancel.hide();
        self.buttons.play.hide();

        self.info.local_size.hide();
        self.info.size_separator.hide();

        self.buttons.download.show();
    }

    fn update_progress(&self, local_size: &str, fraction: f64) {
        self.info.local_size.set_text(local_size);
        self.progressbar.set_fraction(fraction);
    }

    /// Change the state of the `EpisodeWidget`.
    ///
    /// Function Flowchart:
    ///
    /// -------------------       --------------
    /// | Is the Episode  |  YES  |   State:   |
    /// | currently being | ----> | InProgress |
    /// |   downloaded?   |       |            |
    /// -------------------       --------------
    ///         |
    ///         | NO
    ///         |
    ///        \_/
    /// -------------------       --------------
    /// | is the episode  |  YES  |   State:   |
    /// |   downloaded    | ----> |  Playable  |
    /// |    already?     |       |            |
    /// -------------------       --------------
    ///         |
    ///         | NO
    ///         |
    ///        \_/
    /// -------------------
    /// |     State:      |
    /// |   ToDownload    |
    /// -------------------
    fn determine_buttons_state(
        widget: &Rc<Self>,
        episode: &EpisodeWidgetQuery,
        sender: &Sender<Action>,
    ) -> Result<(), Error> {
        // Reset the buttons state no matter the glade file.
        // This is just to make it easier to port to relm in the future.
        widget.buttons.cancel.hide();
        widget.buttons.play.hide();
        widget.buttons.download.hide();

        // Check if the episode is being downloaded
        let id = episode.rowid();
        let active_dl = move || -> Result<Option<_>, Error> {
            let m = manager::ACTIVE_DOWNLOADS
                .read()
                .map_err(|_| format_err!("Failed to get a lock on the mutex."))?;

            Ok(m.get(&id).cloned())
        };

        // State: InProggress
        if let Some(prog) = active_dl()? {
            // set a callback that will update the state when the download finishes
            let callback = clone!(widget, sender => move || {
                if let Ok(guard) = manager::ACTIVE_DOWNLOADS.read() {
                    if !guard.contains_key(&id) {
                        if let Ok(ep) = dbqueries::get_episode_widget_from_rowid(id) {
                            Self::determine_buttons_state(&widget, &ep, &sender)
                                .map_err(|err| error!("Error: {}", err))
                                .ok();

                            return glib::Continue(false)
                        }
                    }
                }

                glib::Continue(true)
            });
            gtk::timeout_add(250, callback);

            // Wire the cancel button
            widget
                .buttons
                .cancel
                .connect_clicked(clone!(prog, widget, sender => move |_| {
                    // Cancel the download
                    if let Ok(mut m) = prog.lock() {
                        m.cancel();
                    }

                    // Cancel is not instant so we have to wait a bit
                    timeout_add(50, clone!(widget, sender => move || {
                        if let Ok(thing) = active_dl() {
                            if thing.is_none() {
                                // Recalculate the widget state
                                dbqueries::get_episode_widget_from_rowid(id)
                                    .map_err(From::from)
                                    .and_then(|ep| Self::determine_buttons_state(&widget, &ep, &sender))
                                    .map_err(|err| error!("Error: {}", err))
                                    .ok();

                                return glib::Continue(false)
                            }
                        }

                        glib::Continue(true)
                    }));
            }));

            // Setup a callback that will update the total_size label
            // with the http ContentLength header number rather than
            // relying to the RSS feed.
            update_total_size_callback(&widget, &prog);

            // Setup a callback that will update the progress bar.
            update_progressbar_callback(&widget, &prog, id);

            // Change the widget layout/state
            widget.state_prog();

            return Ok(());
        }

        // State: Playable
        if episode.local_uri().is_some() {
            // Change the widget layout/state
            widget.state_playable();

            // Wire the play button
            widget
                .buttons
                .play
                .connect_clicked(clone!(widget, sender => move |_| {
                    if let Ok(mut ep) = dbqueries::get_episode_widget_from_rowid(id) {
                        on_play_bttn_clicked(&widget, &mut ep, &sender)
                            .map_err(|err| error!("Error: {}", err))
                            .ok();
                    }
                }));

            return Ok(());
        }

        // State: ToDownload
        // Wire the download button
        widget
            .buttons
            .download
            .connect_clicked(clone!(widget, sender => move |dl| {
                // Make the button insensitive so it won't be pressed twice
                dl.set_sensitive(false);
                if let Ok(ep) = dbqueries::get_episode_widget_from_rowid(id) {
                    on_download_clicked(&ep, &sender)
                        .and_then(|_| {
                            info!("Donwload started succesfully.");
                            Self::determine_buttons_state(&widget, &ep, &sender)
                        })
                        .map_err(|err| error!("Error: {}", err))
                        .ok();
                }

                // Restore sensitivity after operations above complete
                dl.set_sensitive(true);
            }));

        // Change the widget state into `ToDownload`
        widget.state_download();

        Ok(())
    }
}

fn on_download_clicked(ep: &EpisodeWidgetQuery, sender: &Sender<Action>) -> Result<(), Error> {
    let pd = dbqueries::get_podcast_from_id(ep.podcast_id())?;
    let download_fold = get_download_folder(&pd.title())?;

    // Start a new download.
    manager::add(ep.rowid(), download_fold)?;

    // Update Views
    sender
        .send(Action::RefreshEpisodesViewBGR)
        .map_err(From::from)
}

fn on_play_bttn_clicked(
    widget: &Rc<EpisodeWidget>,
    episode: &mut EpisodeWidgetQuery,
    sender: &Sender<Action>,
) -> Result<(), Error> {
    open_uri(episode.rowid())?;
    episode.set_played_now()?;

    widget.info.set_title(&episode);
    sender
        .send(Action::RefreshEpisodesViewBGR)
        .map_err(From::from)
}

fn open_uri(rowid: i32) -> Result<(), Error> {
    let uri = dbqueries::get_episode_local_uri_from_id(rowid)?
        .ok_or_else(|| format_err!("Expected Some found None."))?;

    if Path::new(&uri).exists() {
        info!("Opening {}", uri);
        open::that(&uri)?;
    } else {
        bail!("File \"{}\" does not exist.", uri);
    }

    Ok(())
}

// Setup a callback that will update the progress bar.
#[inline]
#[cfg_attr(feature = "cargo-clippy", allow(if_same_then_else))]
fn update_progressbar_callback(
    widget: &Rc<EpisodeWidget>,
    prog: &Arc<Mutex<manager::Progress>>,
    episode_rowid: i32,
) {
    let callback = clone!(widget, prog => move || {
        progress_bar_helper(&widget, &prog, episode_rowid)
            .unwrap_or(glib::Continue(false))
    });
    timeout_add(100, callback);
}

#[allow(if_same_then_else)]
fn progress_bar_helper(
    widget: &Rc<EpisodeWidget>,
    prog: &Arc<Mutex<manager::Progress>>,
    episode_rowid: i32,
) -> Result<glib::Continue, Error> {
    let (fraction, downloaded) = match prog.try_lock() {
        Ok(guard) => (guard.get_fraction(), guard.get_downloaded()),
        Err(TryLockError::WouldBlock) => return Ok(glib::Continue(true)),
        Err(TryLockError::Poisoned(_)) => return Err(format_err!("Progress Mutex is poisoned")),
    };

    // I hate floating points.
    // Update the progress_bar.
    if (fraction >= 0.0) && (fraction <= 1.0) && (!fraction.is_nan()) {
        // Update local_size label
        let size = downloaded
            .file_size(SIZE_OPTS.clone())
            .map_err(|err| format_err!("{}", err))?;

        widget.update_progress(&size, fraction);
    }

    // info!("Fraction: {}", progress_bar.get_fraction());
    // info!("Fraction: {}", fraction);

    // Check if the download is still active
    let active = match manager::ACTIVE_DOWNLOADS.read() {
        Ok(guard) => guard.contains_key(&episode_rowid),
        Err(_) => return Err(format_err!("Failed to get a lock on the mutex.")),
    };

    if (fraction >= 1.0) && (!fraction.is_nan()) {
        Ok(glib::Continue(false))
    } else if !active {
        Ok(glib::Continue(false))
    } else {
        Ok(glib::Continue(true))
    }
}

// Setup a callback that will update the total_size label
// with the http ContentLength header number rather than
// relying to the RSS feed.
#[inline]
fn update_total_size_callback(widget: &Rc<EpisodeWidget>, prog: &Arc<Mutex<manager::Progress>>) {
    let callback = clone!(prog, widget => move || {
        total_size_helper(&widget, &prog).unwrap_or(glib::Continue(true))
    });
    timeout_add(100, callback);
}

fn total_size_helper(
    widget: &Rc<EpisodeWidget>,
    prog: &Arc<Mutex<manager::Progress>>,
) -> Result<glib::Continue, Error> {
    // Get the total_bytes.
    let total_bytes = match prog.try_lock() {
        Ok(guard) => guard.get_total_size(),
        Err(TryLockError::WouldBlock) => return Ok(glib::Continue(true)),
        Err(TryLockError::Poisoned(_)) => return Err(format_err!("Progress Mutex is poisoned")),
    };

    debug!("Total Size: {}", total_bytes);
    if total_bytes != 0 {
        // Update the total_size label
        widget.info.set_size(Some(total_bytes as i32));

        // Do not call again the callback
        Ok(glib::Continue(false))
    } else {
        Ok(glib::Continue(true))
    }
}

// fn on_delete_bttn_clicked(episode_id: i32) -> Result<(), Error> {
//     let mut ep = dbqueries::get_episode_from_rowid(episode_id)?.into();
//     delete_local_content(&mut ep).map_err(From::from).map(|_| ())
// }
