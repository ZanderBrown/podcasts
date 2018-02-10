use diesel;
use diesel::prelude::*;

use ammonia;
use rss;

use errors::DataError;
use models::{Index, Insert, Update};
use models::Podcast;
use schema::podcast;

use database::connection;
use dbqueries;
use utils::{replace_extra_spaces, url_cleaner};

#[derive(Insertable, AsChangeset)]
#[table_name = "podcast"]
#[derive(Debug, Clone, Default, Builder, PartialEq)]
#[builder(default)]
#[builder(derive(Debug))]
#[builder(setter(into))]
pub(crate) struct NewPodcast {
    title: String,
    link: String,
    description: String,
    image_uri: Option<String>,
    source_id: i32,
}

impl Insert<(), DataError> for NewPodcast {
    fn insert(&self) -> Result<(), DataError> {
        use schema::podcast::dsl::*;
        let db = connection();
        let con = db.get()?;

        diesel::insert_into(podcast)
            .values(self)
            .execute(&con)
            .map(|_| ())
            .map_err(From::from)
    }
}

impl Update<(), DataError> for NewPodcast {
    fn update(&self, podcast_id: i32) -> Result<(), DataError> {
        use schema::podcast::dsl::*;
        let db = connection();
        let con = db.get()?;

        info!("Updating {}", self.title);
        diesel::update(podcast.filter(id.eq(podcast_id)))
            .set(self)
            .execute(&con)
            .map(|_| ())
            .map_err(From::from)
    }
}

// TODO: Maybe return an Enum<Action(Resut)> Instead.
// It would make unti testing better too.
impl Index<(), DataError> for NewPodcast {
    fn index(&self) -> Result<(), DataError> {
        let exists = dbqueries::podcast_exists(self.source_id)?;

        if exists {
            let other = dbqueries::get_podcast_from_source_id(self.source_id)?;

            if self != &other {
                self.update(other.id())
            } else {
                Ok(())
            }
        } else {
            self.insert()
        }
    }
}

impl PartialEq<Podcast> for NewPodcast {
    fn eq(&self, other: &Podcast) -> bool {
        (self.link() == other.link()) && (self.title() == other.title())
            && (self.image_uri() == other.image_uri())
            && (self.description() == other.description())
            && (self.source_id() == other.source_id())
    }
}

impl NewPodcast {
    /// Parses a `rss::Channel` into a `NewPodcast` Struct.
    pub(crate) fn new(chan: &rss::Channel, source_id: i32) -> NewPodcast {
        let title = chan.title().trim();

        // Prefer itunes summary over rss.description since many feeds put html into
        // rss.description.
        let summary = chan.itunes_ext().map(|s| s.summary()).and_then(|s| s);
        let description = if let Some(sum) = summary {
            replace_extra_spaces(&ammonia::clean(sum))
        } else {
            replace_extra_spaces(&ammonia::clean(chan.description()))
        };

        let link = url_cleaner(chan.link());
        let x = chan.itunes_ext().map(|s| s.image());
        let image_uri = if let Some(img) = x {
            img.map(|s| s.to_owned())
        } else {
            chan.image().map(|foo| foo.url().to_owned())
        };

        NewPodcastBuilder::default()
            .title(title)
            .description(description)
            .link(link)
            .image_uri(image_uri)
            .source_id(source_id)
            .build()
            .unwrap()
    }

    // Look out for when tryinto lands into stable.
    pub(crate) fn to_podcast(&self) -> Result<Podcast, DataError> {
        self.index()?;
        dbqueries::get_podcast_from_source_id(self.source_id).map_err(From::from)
    }
}

// Ignore the following geters. They are used in unit tests mainly.
impl NewPodcast {
    #[allow(dead_code)]
    pub(crate) fn source_id(&self) -> i32 {
        self.source_id
    }

    pub(crate) fn title(&self) -> &str {
        &self.title
    }

    pub(crate) fn link(&self) -> &str {
        &self.link
    }

    pub(crate) fn description(&self) -> &str {
        &self.description
    }

    pub(crate) fn image_uri(&self) -> Option<&str> {
        self.image_uri.as_ref().map(|s| s.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // use tokio_core::reactor::Core;

    use rss::Channel;

    use database::truncate_db;
    use models::{NewPodcastBuilder, Save};

    use std::fs::File;
    use std::io::BufReader;

    // Pre-built expected NewPodcast structs.
    lazy_static!{
        static ref EXPECTED_INTERCEPTED: NewPodcast = {
            let descr = "The people behind The Intercept’s fearless reporting and incisive \
                         commentary—Jeremy Scahill, Glenn Greenwald, Betsy Reed and others—discuss \
                         the crucial issues of our time: national security, civil liberties, foreign \
                         policy, and criminal justice. Plus interviews with artists, thinkers, and \
                         newsmakers who challenge our preconceptions about the world we live in.";

            NewPodcastBuilder::default()
                .title("Intercepted with Jeremy Scahill")
                .link("https://theintercept.com/podcasts")
                .description(descr)
                .image_uri(Some(String::from(
                    "http://static.megaphone.fm/podcasts/d5735a50-d904-11e6-8532-73c7de466ea6/image/\
                     uploads_2F1484252190700-qhn5krasklbce3dh-a797539282700ea0298a3a26f7e49b0b_\
                     2FIntercepted_COVER%2B_281_29.png")
                ))
                .source_id(42)
                .build()
                .unwrap()
        };

        static ref EXPECTED_LUP: NewPodcast = {
            let descr = "An open show powered by community LINUX Unplugged takes the best attributes \
                         of open collaboration and focuses them into a weekly lifestyle show about \
                         Linux.";

            NewPodcastBuilder::default()
                .title("LINUX Unplugged Podcast")
                .link("http://www.jupiterbroadcasting.com/")
                .description(descr)
                .image_uri(Some(String::from(
                    "http://www.jupiterbroadcasting.com/images/LASUN-Badge1400.jpg",
                )))
                .source_id(42)
                .build()
                .unwrap()
        };

        static ref EXPECTED_TIPOFF: NewPodcast = {
            let desc = "Welcome to The Tip Off- the podcast where we take you behind the scenes of \
                        some of the best investigative journalism from recent years. Each episode \
                        we’ll be digging into an investigative scoop- hearing from the journalists \
                        behind the work as they tell us about the leads, the dead-ends and of course, \
                        the tip offs. There’ll be car chases, slammed doors, terrorist cells, \
                        meetings in dimly lit bars and cafes, wrangling with despotic regimes and \
                        much more. So if you’re curious about the fun, complicated detective work \
                        that goes into doing great investigative journalism- then this is the podcast \
                        for you.";

            NewPodcastBuilder::default()
                .title("The Tip Off")
                .link("http://www.acast.com/thetipoff")
                .description(desc)
                .image_uri(Some(String::from(
                    "https://imagecdn.acast.com/image?h=1500&w=1500&source=http%3A%2F%2Fi1.sndcdn.\
                     com%2Favatars-000317856075-a2coqz-original.jpg",
                )))
                .source_id(42)
                .build()
                .unwrap()

        };

        static ref EXPECTED_STARS: NewPodcast = {
            let descr = "<p>The first audio drama from Tor Labs and Gideon Media, Steal the Stars is \
                         a gripping noir science fiction thriller in 14 episodes: Forbidden love, a \
                         crashed UFO, an alien body, and an impossible heist unlike any ever \
                         attempted - scripted by Mac Rogers, the award-winning playwright and writer \
                         of the multi-million download The Message and LifeAfter.</p>";
            let img =  "https://dfkfj8j276wwv.cloudfront.net/images/2c/5f/a0/1a/2c5fa01a-ae78-4a8c-\
                        b183-7311d2e436c3/b3a4aa57a576bb662191f2a6bc2a436c8c4ae256ecffaff5c4c54fd42e\
                        923914941c264d01efb1833234b52c9530e67d28a8cebbe3d11a4bc0fbbdf13ecdf1c3.jpeg";

            NewPodcastBuilder::default()
                .title("Steal the Stars")
                .link("http://tor-labs.com/")
                .description(descr)
                .image_uri(Some(String::from(img)))
                .source_id(42)
                .build()
                .unwrap()
        };

        static ref EXPECTED_CODE: NewPodcast = {
            let descr = "A podcast about humans and technology. Panelists: Coraline Ada Ehmke, David \
                         Brady, Jessica Kerr, Jay Bobo, Astrid Countee and Sam Livingston-Gray. \
                         Brought to you by @therubyrep.";

            NewPodcastBuilder::default()
                .title("Greater Than Code")
                .link("https://www.greaterthancode.com/")
                .description(descr)
                .image_uri(Some(String::from(
                    "http://www.greaterthancode.com/wp-content/uploads/2016/10/code1400-4.jpg",
                )))
                .source_id(42)
                .build()
                .unwrap()
        };

        static ref UPDATED_DESC_INTERCEPTED: NewPodcast = {
            NewPodcastBuilder::default()
                .title("Intercepted with Jeremy Scahill")
                .link("https://theintercept.com/podcasts")
                .description("New Description")
                .image_uri(Some(String::from(
                    "http://static.megaphone.fm/podcasts/d5735a50-d904-11e6-8532-73c7de466ea6/image/\
                     uploads_2F1484252190700-qhn5krasklbce3dh-a797539282700ea0298a3a26f7e49b0b_\
                     2FIntercepted_COVER%2B_281_29.png")
                ))
                .source_id(42)
                .build()
                .unwrap()
        };
    }

    #[test]
    fn test_new_podcast_intercepted() {
        let file = File::open("tests/feeds/2018-01-20-Intercepted.xml").unwrap();
        let channel = Channel::read_from(BufReader::new(file)).unwrap();

        let pd = NewPodcast::new(&channel, 42);
        assert_eq!(*EXPECTED_INTERCEPTED, pd);
    }

    #[test]
    fn test_new_podcast_lup() {
        let file = File::open("tests/feeds/2018-01-20-LinuxUnplugged.xml").unwrap();
        let channel = Channel::read_from(BufReader::new(file)).unwrap();

        let pd = NewPodcast::new(&channel, 42);
        assert_eq!(*EXPECTED_LUP, pd);
    }

    #[test]
    fn test_new_podcast_thetipoff() {
        let file = File::open("tests/feeds/2018-01-20-TheTipOff.xml").unwrap();
        let channel = Channel::read_from(BufReader::new(file)).unwrap();

        let pd = NewPodcast::new(&channel, 42);
        assert_eq!(*EXPECTED_TIPOFF, pd);
    }

    #[test]
    fn test_new_podcast_steal_the_stars() {
        let file = File::open("tests/feeds/2018-01-20-StealTheStars.xml").unwrap();
        let channel = Channel::read_from(BufReader::new(file)).unwrap();

        let pd = NewPodcast::new(&channel, 42);
        assert_eq!(*EXPECTED_STARS, pd);
    }

    #[test]
    fn test_new_podcast_greater_than_code() {
        let file = File::open("tests/feeds/2018-01-20-GreaterThanCode.xml").unwrap();
        let channel = Channel::read_from(BufReader::new(file)).unwrap();

        let pd = NewPodcast::new(&channel, 42);
        assert_eq!(*EXPECTED_CODE, pd);
    }

    #[test]
    // This maybe could be a doc test on insert.
    fn test_new_podcast_insert() {
        truncate_db().unwrap();
        let file = File::open("tests/feeds/2018-01-20-Intercepted.xml").unwrap();
        let channel = Channel::read_from(BufReader::new(file)).unwrap();

        let npd = NewPodcast::new(&channel, 42);
        npd.insert().unwrap();
        let pd = dbqueries::get_podcast_from_source_id(42).unwrap();

        assert_eq!(npd, pd);
        assert_eq!(*EXPECTED_INTERCEPTED, npd);
        assert_eq!(&*EXPECTED_INTERCEPTED, &pd);
    }

    #[test]
    // TODO: Add more test/checks
    // Currently there's a test that only checks new description or title.
    // If you have time and want to help, implement the test for the other fields too.
    fn test_new_podcast_update() {
        truncate_db().unwrap();
        let old = EXPECTED_INTERCEPTED.to_podcast().unwrap();

        let updated = &*UPDATED_DESC_INTERCEPTED;
        updated.update(old.id()).unwrap();
        let mut new = dbqueries::get_podcast_from_source_id(42).unwrap();

        assert_ne!(old, new);
        assert_eq!(old.id(), new.id());
        assert_eq!(old.source_id(), new.source_id());
        assert_eq!(updated, &new);
        assert_ne!(updated, &old);

        // Chech that the update does not override user preferences.
        new.set_archive(true);
        new.save().unwrap();

        let new2 = dbqueries::get_podcast_from_source_id(42).unwrap();
        assert_eq!(true, new2.archive());
    }

    #[test]
    fn test_new_podcast_index() {
        truncate_db().unwrap();

        // First insert
        assert!(EXPECTED_INTERCEPTED.index().is_ok());
        // Second identical, This should take the early return path
        assert!(EXPECTED_INTERCEPTED.index().is_ok());
        // Get the podcast
        let old = dbqueries::get_podcast_from_source_id(42).unwrap();
        // Assert that NewPodcast is equal to the Indexed one
        assert_eq!(&*EXPECTED_INTERCEPTED, &old);

        let updated = &*UPDATED_DESC_INTERCEPTED;

        // Update the podcast
        assert!(updated.index().is_ok());
        // Get the new Podcast
        let new = dbqueries::get_podcast_from_source_id(42).unwrap();
        // Assert it's diff from the old one.
        assert_ne!(new, old);
        assert_eq!(new.id(), old.id());
        assert_eq!(new.source_id(), old.source_id());
    }

    #[test]
    fn test_to_podcast() {
        // Assert insert() produces the same result that you would get with to_podcast()
        truncate_db().unwrap();
        EXPECTED_INTERCEPTED.insert().unwrap();
        let old = dbqueries::get_podcast_from_source_id(42).unwrap();
        let pd = EXPECTED_INTERCEPTED.to_podcast().unwrap();
        assert_eq!(old, pd);

        // Same as above, diff order
        truncate_db().unwrap();
        let pd = EXPECTED_INTERCEPTED.to_podcast().unwrap();
        // This should error as a unique constrain violation
        assert!(EXPECTED_INTERCEPTED.insert().is_err());
        let mut old = dbqueries::get_podcast_from_source_id(42).unwrap();
        assert_eq!(old, pd);

        old.set_archive(true);
        old.save().unwrap();

        // Assert that it does not mess with user preferences
        let pd = UPDATED_DESC_INTERCEPTED.to_podcast().unwrap();
        let old = dbqueries::get_podcast_from_source_id(42).unwrap();
        assert_eq!(old, pd);
        assert_eq!(old.archive(), true);
    }
}