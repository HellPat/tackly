use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Where on the globe: latitude and longitude in degrees, and how many
/// meters the phone said it could be off, when it said.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawGeoPoint", into = "RawGeoPoint")]
pub struct GeoPoint {
    latitude: f64,
    longitude: f64,
    accuracy_meters: Option<f64>,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum GeoPointError {
    #[error("latitude is between -90 and 90 degrees")]
    Latitude,
    #[error("longitude is between -180 and 180 degrees")]
    Longitude,
    #[error("accuracy is a number of meters, not negative")]
    Accuracy,
    #[error("coordinates are written as latitude,longitude")]
    NotCoordinates,
}

impl GeoPoint {
    pub fn new(
        latitude: f64,
        longitude: f64,
        accuracy_meters: Option<f64>,
    ) -> Result<Self, GeoPointError> {
        if !(-90.0..=90.0).contains(&latitude) {
            return Err(GeoPointError::Latitude);
        }
        if !(-180.0..=180.0).contains(&longitude) {
            return Err(GeoPointError::Longitude);
        }
        if accuracy_meters.is_some_and(|meters| !(meters >= 0.0 && meters.is_finite())) {
            return Err(GeoPointError::Accuracy);
        }
        Ok(Self {
            latitude,
            longitude,
            accuracy_meters,
        })
    }

    pub fn latitude(&self) -> f64 {
        self.latitude
    }

    pub fn longitude(&self) -> f64 {
        self.longitude
    }

    pub fn accuracy_meters(&self) -> Option<f64> {
        self.accuracy_meters
    }
}

/// `52.52,13.405`: latitude, longitude.
impl FromStr for GeoPoint {
    type Err = GeoPointError;

    fn from_str(text: &str) -> Result<Self, GeoPointError> {
        let (latitude, longitude) = text.split_once(',').ok_or(GeoPointError::NotCoordinates)?;
        let number = |part: &str| {
            part.trim()
                .parse::<f64>()
                .map_err(|_| GeoPointError::NotCoordinates)
        };
        Self::new(number(latitude)?, number(longitude)?, None)
    }
}

/// How a point is written in an event.
#[derive(Serialize, Deserialize)]
struct RawGeoPoint {
    latitude: f64,
    longitude: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    accuracy_meters: Option<f64>,
}

impl TryFrom<RawGeoPoint> for GeoPoint {
    type Error = GeoPointError;

    fn try_from(raw: RawGeoPoint) -> Result<Self, GeoPointError> {
        Self::new(raw.latitude, raw.longitude, raw.accuracy_meters)
    }
}

impl From<GeoPoint> for RawGeoPoint {
    fn from(point: GeoPoint) -> Self {
        Self {
            latitude: point.latitude,
            longitude: point.longitude,
            accuracy_meters: point.accuracy_meters,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_on_the_globe_are_points() {
        assert!(GeoPoint::new(48.8752, 9.3775, Some(12.0)).is_ok());
        assert!(GeoPoint::new(-90.0, 180.0, None).is_ok());
    }

    #[test]
    fn points_off_the_globe_are_not() {
        assert_eq!(GeoPoint::new(90.1, 0.0, None), Err(GeoPointError::Latitude));
        assert_eq!(
            GeoPoint::new(0.0, -180.5, None),
            Err(GeoPointError::Longitude)
        );
        assert_eq!(
            GeoPoint::new(f64::NAN, 0.0, None),
            Err(GeoPointError::Latitude)
        );
        assert_eq!(
            GeoPoint::new(0.0, 0.0, Some(-1.0)),
            Err(GeoPointError::Accuracy)
        );
    }

    #[test]
    fn coordinates_can_be_typed() {
        let point: GeoPoint = " 52.52 , 13.405 ".parse().expect("coordinates");
        assert_eq!((point.latitude(), point.longitude()), (52.52, 13.405));
        assert_eq!(
            "52.52".parse::<GeoPoint>(),
            Err(GeoPointError::NotCoordinates)
        );
        assert_eq!(
            "a,b".parse::<GeoPoint>(),
            Err(GeoPointError::NotCoordinates)
        );
        assert_eq!("100,0".parse::<GeoPoint>(), Err(GeoPointError::Latitude));
    }

    #[test]
    fn an_event_with_a_point_off_the_globe_is_refused() {
        assert!(serde_json::from_str::<GeoPoint>(r#"{"latitude":1.0,"longitude":2.0}"#).is_ok());
        assert!(serde_json::from_str::<GeoPoint>(r#"{"latitude":91.0,"longitude":2.0}"#).is_err());
    }
}
