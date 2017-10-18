use gtk;
use gtk::prelude::*;
use gdk_pixbuf::Pixbuf;

use hammond_downloader::downloader;
use diesel::prelude::*;

use std::sync::{Arc, Mutex};

use widgets::podcast::*;

pub fn populate_podcasts_flowbox(
    db: Arc<Mutex<SqliteConnection>>,
    stack: gtk::Stack,
    flowbox: gtk::FlowBox,
) {
    let tempdb = db.lock().unwrap();
    let pd_model = podcast_liststore(&tempdb);
    drop(tempdb);

    // Get a ListStore iterator at the first element.
    let iter = pd_model.get_iter_first().unwrap();

    loop {
        let title = pd_model.get_value(&iter, 1).get::<String>().unwrap();
        let description = pd_model.get_value(&iter, 2).get::<String>().unwrap();
        let image_uri = pd_model.get_value(&iter, 4).get::<String>();

        let imgpath = downloader::cache_image(&title, image_uri.as_ref().map(|s| s.as_str()));

        let pixbuf = if let Some(i) = imgpath {
            Pixbuf::new_from_file_at_scale(&i, 200, 200, true).ok()
        } else {
            None
        };

        let f = create_flowbox_child(&title, pixbuf.clone());

        let stack_clone = stack.clone();
        let db_clone = db.clone();

        f.connect_activate(move |_| {
            let pdw = stack_clone.get_child_by_name("pdw").unwrap();
            stack_clone.remove(&pdw);
            let pdw = podcast_widget(
                db_clone.clone(),
                Some(title.as_str()),
                Some(description.as_str()),
                pixbuf.clone(),
            );
            stack_clone.add_named(&pdw, "pdw");
            stack_clone.set_visible_child(&pdw);
            println!("Hello World!, child activated");
        });
        flowbox.add(&f);

        if !pd_model.iter_next(&iter) {
            break;
        }
    }
}
