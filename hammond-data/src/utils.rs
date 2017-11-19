use rayon::prelude::*;
use diesel::prelude::*;
use chrono::prelude::*;

use r2d2;
use diesel::sqlite::SqliteConnection;
use r2d2_diesel::ConnectionManager;

use errors::*;
use dbqueries;
use Database;
use models::Episode;

use std::path::Path;
use std::fs;
use std::sync::Arc;

use DB_PATH;

embed_migrations!("migrations/");

pub type Pool = r2d2::Pool<ConnectionManager<SqliteConnection>>;

pub fn init() -> Result<()> {
    let conn = establish_connection();
    run_migration_on(&conn)
}

pub fn init_pool(db_path: &str) -> Pool {
    let config = r2d2::Config::default();
    let manager = ConnectionManager::<SqliteConnection>::new(db_path);
    r2d2::Pool::new(config, manager).expect("Failed to create pool.")
}

pub fn run_migration_on(connection: &SqliteConnection) -> Result<()> {
    info!("Running DB Migrations...");
    embedded_migrations::run(connection)?;
    // embedded_migrations::run_with_output(connection, &mut std::io::stdout())
    Ok(())
}

pub fn establish_connection() -> SqliteConnection {
    let database_url = DB_PATH.to_str().unwrap();
    SqliteConnection::establish(database_url)
        .expect(&format!("Error connecting to {}", database_url))
}

// TODO: Write unit test.
fn download_checker(db: &Database) -> Result<()> {
    let episodes = {
        let tempdb = db.lock().unwrap();
        dbqueries::get_downloaded_episodes(&tempdb)?
    };

    episodes.into_par_iter().for_each(|mut ep| {
        if !Path::new(ep.local_uri().unwrap()).exists() {
            ep.set_local_uri(None);
            let res = ep.save(&Arc::clone(db));
            if let Err(err) = res {
                error!("Error while trying to update episode: {:#?}", ep);
                error!("Error: {}", err);
            };
        }
    });

    Ok(())
}

// TODO: Write unit test.
fn played_cleaner(db: &Database) -> Result<()> {
    let episodes = {
        let tempdb = db.lock().unwrap();
        dbqueries::get_played_episodes(&tempdb)?
    };

    let now_utc = Utc::now().timestamp() as i32;
    episodes.into_par_iter().for_each(|mut ep| {
        if ep.local_uri().is_some() && ep.played().is_some() {
            let played = ep.played().unwrap();
            // TODO: expose a config and a user set option.
            let limit = played + 172_800; // add 2days in seconds
            if now_utc > limit {
                let e = delete_local_content(&Arc::clone(db), &mut ep);
                if let Err(err) = e {
                    error!("Error while trying to delete file: {:?}", ep.local_uri());
                    error!("Error: {}", err);
                } else {
                    info!("Episode {:?} was deleted succesfully.", ep.title());
                };
            }
        }
    });
    Ok(())
}

// TODO: Write unit test.
pub fn delete_local_content(db: &Database, ep: &mut Episode) -> Result<()> {
    if ep.local_uri().is_some() {
        let uri = ep.local_uri().unwrap().to_owned();
        if Path::new(&uri).exists() {
            let res = fs::remove_file(&uri);
            if res.is_ok() {
                ep.set_local_uri(None);
                ep.save(db)?;
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

pub fn set_played_now(db: &Database, ep: &mut Episode) -> Result<()> {
    let epoch = Utc::now().timestamp() as i32;
    ep.set_played(Some(epoch));
    ep.save(db)?;
    Ok(())
}

pub fn checkup(db: &Database) -> Result<()> {
    download_checker(db)?;
    played_cleaner(db)?;
    Ok(())
}
