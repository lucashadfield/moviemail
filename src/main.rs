use std::fs::{read_to_string, write};
use std::collections::{HashMap, HashSet};
use core::option::Option;
use serde_derive::{Deserialize, Serialize};
use futures::future::join_all;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};

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
    let url = format!("https://api.themoviedb.org/3/person/{director_id}/movie_credits?api_key={api_key}&language=en-US");
    let resp = reqwest::get(url).await.expect("error fetching from tmdb").text().await.unwrap();
    let mut credits: Credits = serde_json::from_str(&*resp).expect("error deserializing movie credits");

    for credit in &mut credits.crew {
        credit.director_name = Some(director_name.clone());
    }

    return credits.crew;
}

fn create_message_body(movies: Vec<Movie>) -> String {
    let mut message = String::new();
    for movie in movies {
        let title = movie.title;
        let director = movie.director_name.unwrap();
        message.push_str(&*format!("{title} - {director}\n"));
    }
    return message;
}

fn create_email(movies: Vec<Movie>, to: String, from: String, subject: String) -> Message {
    return Message::builder()
        .to(to.parse().unwrap())
        .from(from.parse().unwrap())
        .subject(subject)
        .body(create_message_body(movies))
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

    // collect results and filter to just directing roles and movies with release dates
    let movies: Vec<Movie> = join_all(movie_futures)
        .await
        .into_iter()
        .flatten()
        .filter(|m| m.job == Some("Director".to_string()))
        .filter(|m| m.release_date != "".to_string())
        .collect();

    // filter out movies previously archived
    let new_movies: Vec<Movie> = movies
        .clone()
        .into_iter()
        .filter(|m| !archive_set.contains(&m.id))
        .collect();

    if new_movies.len() > 0 {
        if config.dry_run {
            for movie in new_movies {
                println!("{:?}, {:?}, {:?}", movie.title, movie.director_name.unwrap(), movie.release_date)
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
    write_archive(&movies, &config.archive_path);
}
