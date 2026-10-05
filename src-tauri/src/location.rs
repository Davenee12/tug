//! Device location for the weather widget, only when the user asks for it
//! ("Use my location"). Windows decides whether desktop apps may see it.

use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Position {
    pub latitude: f64,
    pub longitude: f64,
}

const DENIED: &str =
    "Windows isn't sharing your location with apps. Turn it on in Settings › Privacy & security › Location, or type a city.";

#[cfg(windows)]
pub fn locate() -> Result<Position, String> {
    use std::time::Duration;
    use windows::Devices::Geolocation::{GeolocationAccessStatus, Geolocator};

    let err = |e: windows::core::Error| e.message().to_string();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .map_err(|e| e.to_string())?;
    rt.block_on(async {
        let access = Geolocator::RequestAccessAsync().map_err(err)?.await.map_err(err)?;
        if access != GeolocationAccessStatus::Allowed {
            return Err(DENIED.to_string());
        }
        let locator = Geolocator::new().map_err(err)?;
        let fix = tokio::time::timeout(Duration::from_secs(15), locator.GetGeopositionAsync().map_err(err)?)
            .await
            .map_err(|_| "Finding your location took too long. Try again, or type a city.".to_string())?
            .map_err(err)?;
        let p = fix
            .Coordinate()
            .map_err(err)?
            .Point()
            .map_err(err)?
            .Position()
            .map_err(err)?;
        Ok(Position {
            latitude: p.Latitude,
            longitude: p.Longitude,
        })
    })
}

#[cfg(not(windows))]
pub fn locate() -> Result<Position, String> {
    Err(DENIED.to_string())
}

#[cfg(all(test, windows))]
mod tests {
    /// Hardware check: `cargo test --lib location -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn locate_on_this_pc() {
        match super::locate() {
            Ok(p) => println!("located: {:.2}, {:.2}", p.latitude, p.longitude),
            Err(e) => panic!("locate failed: {e}"),
        }
    }
}
