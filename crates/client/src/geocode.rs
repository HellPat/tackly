//! Finding a place by name, e.g. "LIDL Winnenden", with address and
//! coordinates. Uses a [Photon](https://photon.komoot.io) server (OpenStreetMap
//! data), which is built for search-as-you-type. The typed text is sent to that
//! server and nothing else is. `TACKLY_GEOCODER` points at another server.

use std::time::Duration;

use anyhow::Result;
use reqwest::Client;
use serde::Deserialize;
use tackly_protocol::{GeoPoint, PlaceLocation};
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
        let server = std::env::var("TACKLY_GEOCODER").unwrap_or_else(|_| DEFAULT_SERVER.into());
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
            name,
            address,
            point: Some(GeoPoint {
                latitude,
                longitude,
                accuracy_meters: None,
            }),
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
        assert_eq!(locations[0].name, "Lidl Winnenden");
        assert_eq!(
            locations[0].address.as_deref(),
            Some("Marbacher Straße 12, 71364 Winnenden")
        );
        let point = locations[0].point.unwrap();
        assert_eq!((point.latitude, point.longitude), (48.8752, 9.3775));
    }
}
