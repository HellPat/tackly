//! Finding a place by name, e.g. "LIDL Winnenden", with address and
//! coordinates. Uses a [Photon](https://photon.komoot.io) server (OpenStreetMap
//! data), which is built for search-as-you-type. The typed text is sent to that
//! server and nothing else is. `TACKLY_GEOCODER` points at another server.

use std::time::Duration;

use anyhow::Result;
use reqwest::Client;
use serde::Deserialize;
use tackly_domain::{GeoPoint, PlaceLocation};
use uuid::Uuid;

const DEFAULT_SERVER: &str = "https://photon.komoot.io/api/";
const MAX_RESULTS: usize = 5;

#[derive(Clone, Debug)]
pub struct Geocoder {
    http: Client,
    server: String,
}

impl Geocoder {
    pub fn from_env() -> Result<Self> {
        Self::new(std::env::var("TACKLY_GEOCODER").unwrap_or_else(|_| DEFAULT_SERVER.into()))
    }

    /// Searching on the Photon server at `server`.
    pub fn new(server: String) -> Result<Self> {
        Ok(Self {
            http: Client::builder()
                .connect_timeout(Duration::from_secs(3))
                .timeout(Duration::from_secs(6))
                .build()?,
            server,
        })
    }

    /// Up to five matches for what the person typed. Fails when offline.
    pub async fn search(&self, text: &str) -> Result<Vec<PlaceLocation>> {
        let found: Found = self
            .http
            .get(&self.server)
            .query(&[("q", text), ("limit", &MAX_RESULTS.to_string())])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(found
            .features
            .into_iter()
            .filter_map(Feature::into_location)
            .collect())
    }
}

#[derive(Deserialize)]
struct Found {
    features: Vec<Feature>,
}

#[derive(Deserialize)]
struct Feature {
    geometry: Geometry,
    properties: Properties,
}

#[derive(Deserialize)]
struct Geometry {
    /// Longitude first, then latitude.
    coordinates: Vec<f64>,
}

#[derive(Default, Deserialize)]
struct Properties {
    name: Option<String>,
    street: Option<String>,
    housenumber: Option<String>,
    postcode: Option<String>,
    city: Option<String>,
}

impl Feature {
    fn into_location(self) -> Option<PlaceLocation> {
        let [longitude, latitude, ..] = self.geometry.coordinates[..] else {
            return None;
        };
        let properties = self.properties;
        let street = match (&properties.street, &properties.housenumber) {
            (Some(street), Some(number)) => Some(format!("{street} {number}")),
            (street, _) => street.clone(),
        };
        let town = match (&properties.postcode, &properties.city) {
            (Some(code), Some(city)) => Some(format!("{code} {city}")),
            (code, city) => city.clone().or_else(|| code.clone()),
        };
        let address: Vec<String> = [street, town].into_iter().flatten().collect();
        let address = (!address.is_empty()).then(|| address.join(", "));
        let name = properties
            .name
            .or_else(|| address.clone())
            .filter(|name| !name.is_empty())?;
        let name = match &properties.city {
            Some(city) if !name.contains(city.as_str()) => format!("{name} {city}"),
            _ => name,
        };
        Some(PlaceLocation {
            id: Uuid::now_v7(),
            name: name.parse().ok()?,
            address,
            point: Some(GeoPoint::new(latitude, longitude, None).ok()?),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_photon_answer_becomes_locations_with_address_and_coordinates() {
        let answer = r#"{"features":[
            {"geometry":{"coordinates":[9.3775,48.8752],"type":"Point"},
             "properties":{"name":"Lidl","street":"Marbacher Straße","housenumber":"12","postcode":"71364","city":"Winnenden"}},
            {"geometry":{"coordinates":[]},"properties":{"name":"Broken"}}]}"#;
        let found: Found = serde_json::from_str(answer).unwrap();
        let locations: Vec<_> = found
            .features
            .into_iter()
            .filter_map(Feature::into_location)
            .collect();
        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].name.to_string(), "Lidl Winnenden");
        assert_eq!(
            locations[0].address.as_deref(),
            Some("Marbacher Straße 12, 71364 Winnenden")
        );
        let point = locations[0].point.unwrap();
        assert_eq!((point.latitude(), point.longitude()), (48.8752, 9.3775));
    }

    fn feature(properties: serde_json::Value) -> Option<PlaceLocation> {
        let feature: Feature = serde_json::from_value(serde_json::json!({
            "geometry": {"coordinates": [9.3775, 48.8752]},
            "properties": properties,
        }))
        .unwrap();
        feature.into_location()
    }

    #[test]
    fn an_address_is_made_of_what_is_known() {
        let street_only = feature(serde_json::json!({"name": "Kiosk", "street": "Hauptstraße"}));
        assert_eq!(street_only.unwrap().address.as_deref(), Some("Hauptstraße"));
        let postcode_only = feature(serde_json::json!({"name": "Kiosk", "postcode": "71364"}));
        assert_eq!(postcode_only.unwrap().address.as_deref(), Some("71364"));
        let city_only = feature(serde_json::json!({"name": "Kiosk", "city": "Winnenden"}));
        let city_only = city_only.unwrap();
        assert_eq!(city_only.address.as_deref(), Some("Winnenden"));
        assert_eq!(city_only.name.to_string(), "Kiosk Winnenden");
    }

    #[test]
    fn a_name_that_says_its_town_does_not_say_it_twice() {
        let named = feature(serde_json::json!({"name": "Lidl Winnenden", "city": "Winnenden"}));
        assert_eq!(named.unwrap().name.to_string(), "Lidl Winnenden");
    }

    #[test]
    fn without_a_name_the_address_names_it_and_without_either_it_is_skipped() {
        let unnamed = feature(serde_json::json!({"street": "Hauptstraße", "housenumber": "3"}));
        assert_eq!(unnamed.unwrap().name.to_string(), "Hauptstraße 3");
        assert!(feature(serde_json::json!({})).is_none());
        assert!(feature(serde_json::json!({"name": ""})).is_none());
    }

    #[test]
    fn the_default_server_is_photon() {
        assert!(Geocoder::from_env().is_ok());
    }

    #[tokio::test]
    async fn it_asks_the_server_and_fails_when_it_cannot() {
        let server = tackly_testkit::FakeGeocoder::start().unwrap();
        let geocoder = Geocoder::new(server.url()).unwrap();
        let found = geocoder.search("lidl").await.unwrap();
        assert_eq!(found.len(), 2);
        assert!(geocoder.search("nothing").await.unwrap().is_empty());
        drop(server);
        let nowhere = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        let offline = Geocoder::new(format!("http://{nowhere}/api/")).unwrap();
        assert!(offline.search("lidl").await.is_err());
    }
}
