//! Helper utilities for accomplishing various tasks.

use rayon::prelude::*;
use chrono::prelude::*;

use url::{Position, Url};
use itertools::Itertools;

use errors::*;
use dbqueries;
use models::queryables::EpisodeCleanerQuery;

use std::path::Path;
use std::fs;

fn download_checker() -> Result<()> {
    let episodes = dbqueries::get_downloaded_episodes()?;

    episodes
        .into_par_iter()
        .filter(|ep| !Path::new(ep.local_uri().unwrap()).exists())
        .for_each(|mut ep| {
            ep.set_local_uri(None);
            if let Err(err) = ep.save() {
                error!("Error while trying to update episode: {:#?}", ep);
                error!("Error: {}", err);
            };
        });

    Ok(())
}

fn played_cleaner() -> Result<()> {
    let mut episodes = dbqueries::get_played_cleaner_episodes()?;

    let now_utc = Utc::now().timestamp() as i32;
    episodes
        .par_iter_mut()
        .filter(|ep| ep.local_uri().is_some() && ep.played().is_some())
        .for_each(|ep| {
            // TODO: expose a config and a user set option.
            // Chnage the test too when exposed
            let limit = ep.played().unwrap() + 172_800; // add 2days in seconds
            if now_utc > limit {
                if let Err(err) = delete_local_content(ep) {
                    error!("Error while trying to delete file: {:?}", ep.local_uri());
                    error!("Error: {}", err);
                } else {
                    info!("Episode {:?} was deleted succesfully.", ep.local_uri());
                };
            }
        });
    Ok(())
}

/// Check `ep.local_uri` field and delete the file it points to.
pub fn delete_local_content(ep: &mut EpisodeCleanerQuery) -> Result<()> {
    if ep.local_uri().is_some() {
        let uri = ep.local_uri().unwrap().to_owned();
        if Path::new(&uri).exists() {
            let res = fs::remove_file(&uri);
            if res.is_ok() {
                ep.set_local_uri(None);
                ep.save()?;
            } else {
                error!("Error while trying to delete file: {}", uri);
                error!("Error: {}", res.unwrap_err());
            };
        }
    } else {
        error!(
            "Something went wrong evaluating the following path: {:?}",
            ep.local_uri(),
        );
    }
    Ok(())
}

/// Database cleaning tasks.
///
/// Runs a download checker which looks for `Episode.local_uri` entries that
/// doesn't exist and sets them to None
///
/// Runs a cleaner for played Episode's that are pass the lifetime limit and
/// scheduled for removal.
pub fn checkup() -> Result<()> {
    download_checker()?;
    played_cleaner()?;
    Ok(())
}

/// Remove fragment identifiers and query pairs from a URL
/// If url parsing fails, return's a trimmed version of the original input.
pub fn url_cleaner(s: &str) -> String {
    // Copied from the cookbook.
    // https://rust-lang-nursery.github.io/rust-cookbook/net.html
    // #remove-fragment-identifiers-and-query-pairs-from-a-url
    match Url::parse(s) {
        Ok(parsed) => parsed[..Position::AfterPath].to_owned(),
        _ => s.trim().to_owned(),
    }
}

/// Helper functions that strips extra spaces and newlines and ignores the tabs.
#[allow(match_same_arms)]
pub fn replace_extra_spaces(s: &str) -> String {
    s.trim()
        .chars()
        .filter(|ch| *ch != '\t')
        .coalesce(|current, next| match (current, next) {
            ('\n', '\n') => Ok('\n'),
            ('\n', ' ') => Ok('\n'),
            (' ', '\n') => Ok('\n'),
            (' ', ' ') => Ok(' '),
            (_, _) => Err((current, next)),
        })
        .collect::<String>()
}

#[cfg(test)]
mod tests {
    extern crate tempdir;

    use super::*;
    use database::{connection, truncate_db};
    use models::insertables::NewEpisodeBuilder;
    use self::tempdir::TempDir;
    use std::fs::File;
    use std::io::Write;

    fn helper_db() -> TempDir {
        // Clean the db
        truncate_db().unwrap();
        // Setup tmp file stuff
        let tmp_dir = TempDir::new("hammond_test").unwrap();
        let valid_path = tmp_dir.path().join("virtual_dl.mp3");
        let bad_path = tmp_dir.path().join("invalid_thing.mp3");
        let mut tmp_file = File::create(&valid_path).unwrap();
        writeln!(tmp_file, "Foooo").unwrap();

        // Setup episodes
        let db = connection();
        let con = db.get().unwrap();
        let n1 = NewEpisodeBuilder::default()
            .title("foo_bar".to_string())
            .podcast_id(0)
            .build()
            .unwrap()
            .into_episode(&con)
            .unwrap();

        let n2 = NewEpisodeBuilder::default()
            .title("bar_baz".to_string())
            .podcast_id(1)
            .build()
            .unwrap()
            .into_episode(&con)
            .unwrap();

        let mut ep1 = dbqueries::get_episode_from_pk(&con, n1.title(), n1.podcast_id()).unwrap();
        let mut ep2 = dbqueries::get_episode_from_pk(&con, n2.title(), n2.podcast_id()).unwrap();
        ep1.set_local_uri(Some(valid_path.to_str().unwrap()));
        ep2.set_local_uri(Some(bad_path.to_str().unwrap()));

        drop(con);
        ep1.save().unwrap();
        ep2.save().unwrap();

        tmp_dir
    }

    #[test]
    fn test_download_checker() {
        let tmp_dir = helper_db();
        download_checker().unwrap();
        let episodes = dbqueries::get_downloaded_episodes().unwrap();
        let valid_path = tmp_dir.path().join("virtual_dl.mp3");

        assert_eq!(episodes.len(), 1);
        assert_eq!(
            Some(valid_path.to_str().unwrap()),
            episodes.first().unwrap().local_uri()
        );

        let _tmp_dir = helper_db();
        download_checker().unwrap();
        let episode = {
            let db = connection();
            let con = db.get().unwrap();
            dbqueries::get_episode_from_pk(&con, "bar_baz", 1).unwrap()
        };
        assert!(episode.local_uri().is_none());
    }

    #[test]
    fn test_download_cleaner() {
        let _tmp_dir = helper_db();
        let mut episode: EpisodeCleanerQuery = {
            let db = connection();
            let con = db.get().unwrap();
            dbqueries::get_episode_from_pk(&con, "foo_bar", 0)
                .unwrap()
                .into()
        };

        let valid_path = episode.local_uri().unwrap().to_owned();
        delete_local_content(&mut episode).unwrap();
        assert_eq!(Path::new(&valid_path).exists(), false);
    }

    #[test]
    fn test_played_cleaner_expired() {
        let _tmp_dir = helper_db();
        let mut episode = {
            let db = connection();
            let con = db.get().unwrap();
            dbqueries::get_episode_from_pk(&con, "foo_bar", 0).unwrap()
        };
        let now_utc = Utc::now().timestamp() as i32;
        // let limit = now_utc - 172_800;
        let epoch = now_utc - 200_000;
        episode.set_played(Some(epoch));
        episode.save().unwrap();
        let valid_path = episode.local_uri().unwrap().to_owned();

        // This should delete the file
        played_cleaner().unwrap();
        assert_eq!(Path::new(&valid_path).exists(), false);
    }

    #[test]
    fn test_played_cleaner_none() {
        let _tmp_dir = helper_db();
        let mut episode = {
            let db = connection();
            let con = db.get().unwrap();
            dbqueries::get_episode_from_pk(&con, "foo_bar", 0).unwrap()
        };
        let now_utc = Utc::now().timestamp() as i32;
        // limit = 172_800;
        let epoch = now_utc - 20_000;
        episode.set_played(Some(epoch));
        episode.save().unwrap();
        let valid_path = episode.local_uri().unwrap().to_owned();

        // This should not delete the file
        played_cleaner().unwrap();
        assert_eq!(Path::new(&valid_path).exists(), true);
    }

    #[test]
    fn test_url_cleaner() {
        let good_url = "http://traffic.megaphone.fm/FL8608731318.mp3";
        let bad_url = "http://traffic.megaphone.fm/FL8608731318.mp3?updated=1484685184";

        assert_eq!(url_cleaner(bad_url), good_url);
        assert_eq!(url_cleaner(good_url), good_url);
        assert_eq!(url_cleaner(&format!("   {}\t\n", bad_url)), good_url);
    }

    #[test]
    fn test_whitespace() {
        let bad_txt = "1   2   3        4  5";
        let valid_txt = "1 2 3 4 5";

        assert_eq!(replace_extra_spaces(&bad_txt), valid_txt);

        let bad_txt = "1   2   3  \n      4  5\n";
        let valid_txt = "1 2 3\n4 5";

        assert_eq!(replace_extra_spaces(&bad_txt), valid_txt);

        let bad_txt = "1   2   3  \n\n\n    \n  4  5\n";
        let valid_txt = "1 2 3\n4 5";

        assert_eq!(replace_extra_spaces(&bad_txt), valid_txt);
    }
}
