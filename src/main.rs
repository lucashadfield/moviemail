use std::fs::{read_to_string, write};
use std::collections::{HashMap, HashSet};
use core::option::Option;
use serde_derive::{Deserialize, Serialize};
use futures::future::join_all;

#[derive(Deserialize)]
struct Config {
    archive_path: String,
    api_key: String,
    directors: HashMap<String, String>,
}

#[derive(Deserialize, Serialize)]
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
    let config_str = read_to_string(path).unwrap();
    let config: Config = toml::from_str(&*config_str).unwrap();

    return config;
}

fn read_archive(path: &str) -> Vec<Movie> {
    match read_to_string(path) {
        Ok(m) => { serde_json::from_str(&*m).unwrap() }
        Err(_) => { vec![] }
    }
}

fn write_archive(movies: &Vec<Movie>, path: &str) {
    let movies_json = serde_json::to_string(movies).unwrap();
    write(path, movies_json).unwrap();
}

async fn fetch_director_credits(director_id: String, director_name: String, api_key: &String) -> Vec<Movie> {
    let url = format!("https://api.themoviedb.org/3/person/{director_id}/movie_credits?api_key={api_key}&language=en-US");
    let resp = reqwest::get(url).await.unwrap().text().await.unwrap();
    let mut credits: Credits = serde_json::from_str(&*resp).unwrap();

    for credit in &mut credits.crew {
        credit.director_name = Some(director_name.clone());
    }

    return credits.crew;
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

    // write all movies to archive file
    write_archive(&movies, &config.archive_path);

    // filter out movies previously archived
    let movies: Vec<Movie> = movies
        .into_iter()
        .filter(|m| !archive_set.contains(&m.id))
        .collect();

    for movie in movies {
        println!("{:?}, {:?}, {:?}", movie.title, movie.director_name.unwrap(), movie.release_date)
    }
}
