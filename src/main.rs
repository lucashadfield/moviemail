use std::fs::{read_to_string, write};
use std::collections::{HashMap, HashSet};
use core::option::Option;
use serde_derive::{Deserialize, Serialize};
use futures::future::join_all;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use lettre::message::MultiPart;
use shellexpand::tilde;
use log::info;

#[derive(Deserialize)]
struct Config {
    archive_path: String,
    dry_run: bool,
    api_key: String,
    to: String,
    from: String,
    subject: String,
    username: String,
    password: String,
    smtp: String,
    directors: HashMap<String, String>,
}

#[derive(Deserialize, Serialize, Clone)]
struct Movie {
    id: u32,
    title: String,
    overview: String,
    poster_path: Option<String>,
    release_date: String,
    job: Option<String>,
    director_name: Option<String>,
    imdb_id: Option<String>,
}

#[derive(Deserialize)]
struct MovieDetails {
    id: u32,
    imdb_id: Option<String>,
    runtime: Option<u32>,
}

#[derive(Deserialize)]
struct Credits {
    crew: Vec<Movie>,
}

fn read_config(path: &str) -> Config {
    let config_str = read_to_string(path).expect("error reading config.toml");
    let config: Config = toml::from_str(&*config_str).expect("error deserializing config.toml");

    return config;
}

fn read_archive(path: &str) -> Vec<Movie> {
    match read_to_string(path) {
        Ok(m) => { serde_json::from_str(&*m).expect("error reading archive.json") }
        Err(_) => { vec![] }
    }
}

fn write_archive(movies: &Vec<Movie>, path: &str) {
    let movies_json = serde_json::to_string(movies).expect("error in serializing movies for archive");
    write(path, movies_json).expect("error writing archive.json");
}

async fn fetch_director_credits(director_id: String, director_name: String, api_key: &String) -> Vec<Movie> {
    // https://developers.themoviedb.org/3/people/get-person-movie-credits
    let url = format!("https://api.themoviedb.org/3/person/{director_id}/movie_credits?api_key={api_key}&language=en-US");
    let resp = reqwest::get(url).await.expect("error fetching from tmdb").text().await.unwrap();
    let mut credits: Credits = serde_json::from_str(&*resp).expect("error deserializing movie credits");

    for credit in &mut credits.crew {
        credit.director_name = Some(director_name.clone());
    }

    return credits.crew;
}

async fn fetch_movie_details(movie_id: u32, api_key: &String) -> MovieDetails {
    // https://developers.themoviedb.org/3/movies/get-movie-details
    let url = format!("https://api.themoviedb.org/3/movie/{movie_id}?api_key={api_key}&language=en-US");
    let resp = reqwest::get(url).await.expect("error fetching from tmdb").text().await.unwrap();
    let movie_details: MovieDetails = serde_json::from_str(&*resp).expect("error deserializing movie details");
    return movie_details;
}

fn create_message_body(movies: Vec<Movie>) -> (String, String) {
    let mut message_plain = String::new();
    let mut message_html = String::new();
    for movie in movies {
        let title = movie.title;
        let director = movie.director_name.unwrap();

        let link = match movie.imdb_id {
            Some(imdb_id) => { format!("https://www.imdb.com/title/{}", imdb_id) }
            None => { format!("https://www.themoviedb.org/movie/{}", movie.id) }
        };

        message_plain.push_str(&*format!("{} - {} - {}\n", link, title, director));
        message_html.push_str(&*format!("<p><a href=\"{}\">{} - {}</a></p>", link, title, director));
    }
    return (message_plain, message_html);
}

fn create_email(movies: Vec<Movie>, to: String, from: String, subject: String) -> Message {
    let (plain, html) = create_message_body(movies);

    return Message::builder()
        .to(to.parse().unwrap())
        .from(from.parse().unwrap())
        .subject(subject)
        .multipart(MultiPart::alternative_plain_html(
            plain,
            html
        ))
        .unwrap();
}


#[tokio::main]
async fn main() {
    let env = env_logger::Env::default().filter_or("LOG_LEVEL", "info");
    env_logger::init_from_env(env);

    let config_path = &tilde("~/.config/moviemail/config.toml");
    info!("reading config from {}", config_path);
    let config = read_config(&config_path);

    let archive_path = &tilde(&config.archive_path);
    info!("reading archive from {}", archive_path);
    let archive = read_archive(&archive_path);
    let archive_set: HashSet<u32> = archive.into_iter().map(|a| a.id).collect();

    info!("loaded {} movies from archive", archive_set.len());

    // for each director call tmdb async
    let movie_futures = config.directors
        .clone()
        .into_iter()
        .map(|d| fetch_director_credits(d.0, d.1, &config.api_key));

    // collect results and filter to just directing roles and movies with release dates later than 2023
    let mut movies: HashMap<u32, Movie> = join_all(movie_futures)
        .await
        .into_iter()
        .flatten()
        .filter(|m| m.job == Some("Director".to_string()))
        .filter(|m| m.release_date != "".to_string())
        .map(|m| (m.id, m))
        .collect();

    info!("fetched {} movies from {} directors", movies.len(), config.directors.len());

    // filter out movies previously archived
    let mut new_movies: Vec<Movie> = movies
        .values()
        .cloned()
        .filter(|m| !archive_set.contains(&m.id))
        .collect();

    info!("{} unfiltered new movies", new_movies.len());

    // get details for new_movies and store async
    let movie_details_futures = new_movies
        .clone()
        .into_iter()
        .map(|m| fetch_movie_details(m.id, &config.api_key));

    let movie_details: HashMap<u32, MovieDetails> = join_all(movie_details_futures)
        .await
        .into_iter()
        .map(|m| (m.id, m))
        .collect();

    // get details for new_movies and store in HashMap
    let mut invalid_new_movies: HashSet<u32> = HashSet::new();
    for movie in &mut new_movies {
        let details = movie_details.get(&movie.id).unwrap();
        let runtime = details.runtime.unwrap_or(0);

        // if no imdb, ignore it
        match &details.imdb_id {
            Some(imdb_id) => { movie.imdb_id = Some(imdb_id.clone()); }
            None => {
                movies.remove(&movie.id);
                invalid_new_movies.insert(movie.id);
                continue;
            }
        }

        if runtime == 0 {
            // remove from movies
            movies.remove(&movie.id);
        }

        if runtime < 60 {
            // remove from new_movies
            invalid_new_movies.insert(movie.id);
        }
    }

    // remove invalid movies from new_movies
    new_movies = new_movies
        .into_iter()
        .filter(|m| !invalid_new_movies.contains(&m.id))
        .collect();

    info!("{} valid new movies", new_movies.len());

    if new_movies.len() > 0 {
        if config.dry_run {
            for movie in new_movies {
                println!("{:?}, {:?}, {:?}, {:?}", movie.title, movie.director_name.unwrap(), movie.release_date, movie.imdb_id.unwrap_or("".to_string()));
            }
        } else {
            info!("preparing email");
            let email = create_email(new_movies, config.to, config.from, config.subject);
            let creds = Credentials::new(config.username, config.password);

            let mailer = SmtpTransport::relay(&*config.smtp)
                .unwrap()
                .credentials(creds)
                .build();

            match mailer.send(&email) {
                Ok(_) => info!("email sent successfully!"),
                Err(e) => panic!("Could not send email: {:?}", e),
            }
        }
    }

    // write all movies to archive file
    info!("writing {} movies to archive", movies.len());
    write_archive(&movies.values().cloned().collect(), &archive_path);
}