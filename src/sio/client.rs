//! SIO Client
//!
//! The ScaleIO `Client`
//!

use std;
use std::cell::RefCell;
use std::collections::{HashMap, BTreeMap};
use std::fs::File;
use std::io::Read;
use std::sync::{Arc, Mutex};

extern crate core;
extern crate serde;
extern crate serde_json;

extern crate hyper_insecure_https_connector;
extern crate hyper;
use hyper::client;
use hyper::header::{Authorization, Basic, Headers, ContentType, UserAgent};
use hyper::status::StatusCode;

pub struct Client {
    gw: String,
    user: String,
    pass: String,
    token: RefCell<Option<String>>,
}

impl Client {
    pub fn new(gw: String, user: String, pass: String) -> Arc<Mutex<Client>> {
        Arc::new(Mutex::new(Client { gw: gw,
                                     user: user,
                                     pass: pass,
                                     token: RefCell::new(None), }))
    }

    fn connect(&self) -> Result<(), String> {
        let url = format!("https://{}/api/login", &self.gw);
        let mut buf = String::new();
        let mut headers = Headers::new();
        headers.set(Authorization(Basic { username: self.user.to_string(),
                                          password: Some(self.pass.to_string()), }));
        headers.set(UserAgent("sio2prom".to_string()));
        let client = client::Client::with_connector(hyper_insecure_https_connector::insecure_https_connector());

        let mut response = client.get(&url).headers(headers).send().unwrap_or_else(|e| {
            panic!("Failed to contact the ScaleIO REST API {} - {}", url, e);
        });

        if response.status != StatusCode::Ok {
            panic!("Invalid ScaleIO REST API {} Response StatusCode: {}", url, response.status);
        }

        response.read_to_string(&mut buf).unwrap_or_else(|e| {
            panic!("Failed to read ScaleIO REST API response: {}", e);
        });

        if buf.replace('"', "").to_string().is_empty() {
            Err(format!("Could not get auth token from the ScaleIO REST API {}", url))
        } else {
            *self.token.borrow_mut() = Some(buf.replace('"', ""));
            debug!("Token: {}", self.token.borrow().clone().unwrap());
            Ok(())
        }
    }

    fn connect_check(&self) -> Result<(), String> {
        let url = format!("https://{}/api/Configuration", &self.gw);
        let mut headers = Headers::new();
        headers.set(Authorization(Basic { username: self.user.to_string(),
                                          password: Some(self.token.borrow().clone().unwrap()), }));
        headers.set(UserAgent("sio2prom".to_string()));
        let client = client::Client::with_connector(hyper_insecure_https_connector::insecure_https_connector());

        let response = match client.get(&url).headers(headers).send() {
            Err(e) => return Err(e.to_string()),
            Ok(r) => r,
        };

        if response.status != StatusCode::Ok {
            error!("ScaleIO REST API {} Check: {}, requesting a new auth token", url, response.status);
            *self.token.borrow_mut() = None;
            Err(format!("ScaleIO REST API {} Check: {}, requesting a new auth token", url, response.status))
        } else {
            debug!("ScaleIO REST API Check: {}", response.status);
            Ok(())
        }
    }

    pub fn stats(&self) -> BTreeMap<String, serde_json::Value> {
        if self.token.borrow().is_none() {
            self.connect();
        }

        let url = format!("https://{}/api/instances/querySelectedStatistics", &self.gw);
        let mut buf = String::new();
        let mut headers = Headers::new();
        headers.set(ContentType::json());
        headers.set(Authorization(Basic { username: self.user.to_string(),
                                          password: Some(self.token.borrow().clone().unwrap()), }));
        headers.set(UserAgent("sio2prom".to_string()));

        let client = client::Client::with_connector(hyper_insecure_https_connector::insecure_https_connector());
        let query: String = Client::read_json("cfg/metric_query_selection.json").map(|q| serde_json::to_string(&q)).expect("Could not load the query (querySelectedStatistics)").unwrap();

        let mut response = client.post(&url).headers(headers).body(&query).send().unwrap_or_else(|e| {
            panic!("Failed to retrieve stats from the ScaleIO REST API {} - {}", url, e);
        });

        if response.status == StatusCode::Unauthorized {
            warn!("Requesting a new auth token");
            self.connect();
        } else if response.status != StatusCode::Ok {
            panic!("Invalid ScaleIO REST API {} Response StatusCode: {}", url, response.status);
        }

        response.read_to_string(&mut buf).unwrap_or_else(|e| {
            panic!("Failed to read ScaleIO REST API response: {}", e);
        });

        let json: serde_json::Value = serde_json::from_str(&buf).unwrap_or_else(|e| {
            panic!("Failed to parse json: {}", e);
        });

        let data: &BTreeMap<String, serde_json::Value> = json.as_object().unwrap_or_else(|| {
            panic!("Failed deserialize json");
        });

        data.clone()
    }

    pub fn instances(&self) -> BTreeMap<String, serde_json::Value> {
        if self.token.borrow().is_none() {
            self.connect();
        }
        if self.connect_check().is_err() {
            self.connect();
        }

        let url = format!("https://{}/api/instances/", &self.gw);
        let mut buf = String::new();
        let mut headers = Headers::new();
        headers.set(ContentType::json());
        headers.set(Authorization(Basic { username: self.user.to_string(),
                                          password: Some(self.token.borrow().clone().unwrap()), }));
        headers.set(UserAgent("sio2prom".to_string()));

        let client = client::Client::with_connector(hyper_insecure_https_connector::insecure_https_connector());

        let mut response = client.get(&url).headers(headers).send().unwrap_or_else(|e| {
            panic!("Failed to retrieve instances from the ScaleIO REST API {} - {}", url, e);
        });

        if response.status == StatusCode::Unauthorized {
            warn!("Requesting a new auth token");
            self.connect();
        } else if response.status != StatusCode::Ok {
            panic!("Invalid ScaleIO REST API {} Response StatusCode: {}", url, response.status);
        }

        response.read_to_string(&mut buf).unwrap_or_else(|e| {
            panic!("Failed to read ScaleIO REST API response: {}", e);
        });

        let json: serde_json::Value = serde_json::from_str(&buf).unwrap_or_else(|e| {
            panic!("Failed to parse json: {}", e);
        });

        let data: &BTreeMap<String, serde_json::Value> = json.as_object().unwrap_or_else(|| {
            panic!("Failed deserialize json");
        });

        data.clone()
    }

    /// Read json file using `serde_json`
    fn read_json(file: &str) -> Option<BTreeMap<String, serde_json::Value>> {
        match File::open(file) {
            Err(e) => panic!("Failed to open file: {}, {:?}", file, e.kind()),
            Ok(mut f) => {
                let mut content: String = String::new();
                f.read_to_string(&mut content).ok().expect("Error reading file");
                let j: serde_json::Value = serde_json::from_str::<serde_json::Value>(&mut content).expect(&format!("Can't deserialize json file {}", file));
                Some(j.as_object().unwrap().clone())
            },
        }
    }
}
