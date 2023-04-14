use std::fs::{read_to_string, write};
use std::collections::{HashMap, HashSet};
use core::option::Option;
use serde_derive::{Deserialize, Serialize};
use futures::future::join_all;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use lettre::message::MultiPart;

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
    let config = read_config("/home/lucas/.config/moviemail/config.toml");
    let archive = read_archive(&config.archive_path);
    let archive_set: HashSet<u32> = archive.into_iter().map(|a| a.id).collect();

    // for each director call tmdb async
    let movie_futures = config.directors
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
        // .filter(|m| m.release_date >= "2023-01-01".to_string())
        .collect();

    // filter out movies previously archived
    let mut new_movies: Vec<Movie> = movies
        .values()
        .cloned()
        .filter(|m| !archive_set.contains(&m.id))
        .collect();

    // get details for new_movies and store in HashMap
    let mut invalid_new_movies: HashSet<u32> = HashSet::new();
    for movie in &mut new_movies {
        let details = fetch_movie_details(movie.id, &config.api_key).await;
        let runtime = details.runtime.unwrap_or(0);

        if runtime == 0 {
            // remove from movies
            movies.remove(&movie.id);
        }
        if runtime < 60 {
            // remove from new_movies
            invalid_new_movies.insert(movie.id);
        } else {
            // update new_movies with imdb_id
            movie.imdb_id = details.imdb_id.clone();
        }
    }

    // remove invalid movies from new_movies
    new_movies = new_movies.into_iter().filter(|m| !invalid_new_movies.contains(&m.id)).collect();

    if new_movies.len() > 0 {
        if config.dry_run {
            for movie in new_movies {
                println!("{:?}, {:?}, {:?}, {:?}", movie.title, movie.director_name.unwrap(), movie.release_date, movie.imdb_id.unwrap_or("".to_string()));
            }
        } else {
            let email = create_email(new_movies, config.to, config.from, config.subject);
            let creds = Credentials::new(config.username, config.password);

            let mailer = SmtpTransport::relay(&*config.smtp)
                .unwrap()
                .credentials(creds)
                .build();

            match mailer.send(&email) {
                Ok(_) => println!("Email sent successfully!"),
                Err(e) => panic!("Could not send email: {:?}", e),
            }
        }
    } else {
        println!("No new movies")
    }

    // write all movies to archive file
    write_archive(&movies.values().cloned().collect(), &config.archive_path);
}