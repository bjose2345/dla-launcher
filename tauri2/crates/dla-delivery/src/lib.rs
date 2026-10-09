//! Downloads catalog releases from the DLA archive for Install: an HTTPS
//! client for the receiver's read-only downloads listener, reached through
//! the bundled cloudflared forward and trusted only through the DLA upload
//! CA; the manifest-based locator; and the resumable, verified transfer.

mod client;
mod locator;
mod names;
mod transfer;

#[cfg(test)]
mod go_service_tests;
#[cfg(test)]
mod service_tests;

pub use client::{Credentials, DeliveryClient, Dial, Endpoint, Entry, PRIVATE_DOWNLOADS};
pub use locator::{ARCHIVE_ROOT, MANIFEST_NAME, ManifestLocator};
pub use names::local_name;
pub use transfer::{FileTransfer, PART_SUFFIX};
