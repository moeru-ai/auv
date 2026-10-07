//! Runner-owned store of captures that clients address by `CaptureRef`.
//!
//! Captures stay in the Runner that produced them; responses carry a
//! reference and metadata, and `GetCaptureImage` is the only call that moves
//! AUV-produced pixels to a client (see "Image Payloads" in `AGENTS.md`).
//!
//! Beyond holding pixels, the store saves repeated work on them:
//!
//! - identical captures (same pixels and metadata, as from polling an
//!   unchanged window) share one blob;
//! - OCR results and fetched images are cached on the blob;
//! - blobs idle for `cold_after` are packed losslessly (QOI) and unpacked on
//!   the next read.
//!
//! When the store exceeds its budget it gives memory back in order of cost to
//! recover: derived caches first, then packing the least recently used hot
//! blobs, then evicting the least recently used captures.
//!
//! TODO(capture-store-prepared-images): images are cached on first fetch, not
//! prepared at capture time; a capture request hint (for example "a logical
//! JPEG will follow") could encode in the background once a caller needs the
//! first fetch faster.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use auv_api_proto::auv::api::image::v1::EncodedImage;
use image::ImageEncoder as _;

/// Environment variable overriding the store budget, in MiB.
pub(super) const BUDGET_MIB_ENV: &str = "AUV_CAPTURE_STORE_BUDGET_MIB";
/// Environment variable overriding the idle expiry, in seconds.
pub(super) const IDLE_SECONDS_ENV: &str = "AUV_CAPTURE_STORE_IDLE_SECONDS";

/// Store limits. Captures leave the store when they exceed the byte budget
/// (least recently used first) or stay unused longer than `idle`. Pixels
/// unused for `cold_after` are packed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CaptureStoreOptions {
  pub budget_bytes: usize,
  pub idle: Duration,
  pub cold_after: Duration,
}

impl Default for CaptureStoreOptions {
  fn default() -> Self {
    // NOTICE(capture-store-defaults): 512 MiB holds about 20 Retina window
    // captures (~25 MB) or 6 display captures (~80 MB) unpacked, and several
    // times more once packed, enough for capture -> OCR -> click loops. Ten
    // idle minutes keep a long-lived local Runner from holding pixels of
    // finished workflows. The Runner cannot see Runs (the daemon strips Run
    // routing metadata), so expiry is time based.
    // TODO(capture-store-run-release): release a Run's captures when it stops
    // once Runners receive Run identity and stop notifications.
    // NOTICE(capture-store-cold): 30 s covers a capture -> OCR -> fetch
    // sequence; QOI packs a Retina window in ~3-14 ms (release, 2026-10-07)
    // to 70-96% of PNG size, so a later read costs little.
    Self {
      budget_bytes: 512 * 1024 * 1024,
      idle: Duration::from_secs(10 * 60),
      cold_after: Duration::from_secs(30),
    }
  }
}

impl CaptureStoreOptions {
  /// Defaults, overridden by `AUV_CAPTURE_STORE_BUDGET_MIB` and
  /// `AUV_CAPTURE_STORE_IDLE_SECONDS` when they hold positive integers.
  pub(super) fn from_env() -> Self {
    let mut options = Self::default();
    if let Some(mib) = positive_env(BUDGET_MIB_ENV) {
      options.budget_bytes = usize::try_from(mib).unwrap_or(usize::MAX).saturating_mul(1024 * 1024);
    }
    if let Some(seconds) = positive_env(IDLE_SECONDS_ENV) {
      options.idle = Duration::from_secs(seconds);
    }
    options
  }
}

fn positive_env(name: &str) -> Option<u64> {
  std::env::var(name).ok()?.trim().parse::<u64>().ok().filter(|value| *value > 0)
}

/// A region as a request gave it: fractions of the image or a screen
/// rectangle. Captures with equal content share bounds, so equal requests
/// name the same pixels; the cache needs no resolved region and a hit skips
/// reading (or unpacking) the capture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct RegionKey {
  normalized: Option<[u64; 4]>,
  screen: Option<[u64; 4]>,
}

impl RegionKey {
  pub(super) fn new(
    normalized: Option<&auv_api_proto::auv::api::image::v1::NormalizedRect>,
    screen: Option<&auv_api_proto::auv::api::driver::v1::ScreenRect>,
  ) -> Self {
    Self {
      normalized: normalized.map(|rect| {
        [
          rect.x.to_bits(),
          rect.y.to_bits(),
          rect.width.to_bits(),
          rect.height.to_bits(),
        ]
      }),
      screen: screen.map(|rect| {
        [
          rect.x.to_bits(),
          rect.y.to_bits(),
          rect.width.to_bits(),
          rect.height.to_bits(),
        ]
      }),
    }
  }
}

/// What OCR results are cached under: the region and recognition options.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct RecognitionKey {
  region: RegionKey,
  custom_words: Vec<String>,
  recognition_languages: Vec<String>,
}

impl RecognitionKey {
  pub(super) fn new(region: RegionKey, custom_words: &[String], recognition_languages: &[String]) -> Self {
    Self {
      region,
      custom_words: custom_words.to_vec(),
      recognition_languages: recognition_languages.to_vec(),
    }
  }
}

/// What fetched images are cached under: the `GetCaptureImage` shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct ImageKey {
  region: RegionKey,
  max_size: Option<(u32, u32)>,
  encoding: i32,
}

impl ImageKey {
  pub(super) fn new(region: RegionKey, max_size: Option<(u32, u32)>, encoding: i32) -> Self {
    Self {
      region,
      max_size,
      encoding,
    }
  }
}

#[derive(Clone)]
pub(super) struct CaptureStore {
  options: CaptureStoreOptions,
  inner: Arc<Mutex<Inner>>,
}

/// Content hash of a capture's pixels and metadata.
type BlobKey = [u8; 32];

#[derive(Default)]
struct Inner {
  entries: HashMap<String, Entry>,
  blobs: HashMap<BlobKey, Blob>,
  total_bytes: usize,
  next: u64,
}

struct Entry {
  blob: BlobKey,
  last_used: Instant,
}

struct Blob {
  pixels: Pixels,
  /// Entries referring to this blob.
  holders: usize,
  last_used: Instant,
  recognitions: HashMap<RecognitionKey, Arc<auv_driver::TextRecognition>>,
  images: HashMap<ImageKey, Arc<EncodedImage>>,
  /// Bytes of `pixels` plus the derived caches.
  bytes: usize,
}

enum Pixels {
  Hot(Arc<auv_driver::Capture>),
  /// QOI bytes; `meta` is the capture with an empty image.
  Cold {
    meta: auv_driver::Capture,
    qoi: Vec<u8>,
  },
}

impl Pixels {
  fn bytes(&self) -> usize {
    match self {
      Self::Hot(capture) => capture.image.as_raw().len(),
      Self::Cold { qoi, .. } => qoi.len(),
    }
  }
}

impl Blob {
  fn derived_bytes(&self) -> usize {
    self.recognitions.values().map(|recognition| recognition_bytes(recognition)).sum::<usize>()
      + self.images.values().map(|image| image.data.len()).sum::<usize>()
  }
}

/// Approximate heap size of a recognition result.
fn recognition_bytes(recognition: &auv_driver::TextRecognition) -> usize {
  recognition.text.len() + recognition.regions.iter().map(|region| region.text.len() + 64).sum::<usize>()
}

impl CaptureStore {
  pub(super) fn new(options: CaptureStoreOptions) -> Self {
    Self {
      options,
      inner: Arc::new(Mutex::new(Inner::default())),
    }
  }

  /// Stores a capture and returns its reference ID. A capture identical to a
  /// stored one shares its pixels. Memory is then given back until the store
  /// fits its budget; a single capture larger than the whole budget is kept
  /// until the next insert.
  pub(super) fn insert(&self, capture: auv_driver::Capture) -> String {
    self.insert_at(capture, Instant::now())
  }

  fn insert_at(&self, capture: auv_driver::Capture, now: Instant) -> String {
    let key = content_key(&capture);
    let mut inner = self.inner.lock().expect("capture store lock");
    inner.expire(now, self.options.idle);
    match inner.blobs.get_mut(&key) {
      Some(blob) => {
        blob.holders += 1;
        blob.last_used = now;
      }
      None => {
        let pixels = Pixels::Hot(Arc::new(capture));
        let bytes = pixels.bytes();
        inner.total_bytes += bytes;
        inner.blobs.insert(
          key,
          Blob {
            pixels,
            holders: 1,
            last_used: now,
            recognitions: HashMap::new(),
            images: HashMap::new(),
            bytes,
          },
        );
      }
    }
    inner.next += 1;
    // The process ID keeps references from a restarted Runner from colliding.
    let id = format!("cap-{}-{}", std::process::id(), inner.next);
    inner.entries.insert(
      id.clone(),
      Entry {
        blob: key,
        last_used: now,
      },
    );
    inner.fit(self.options.budget_bytes, Some(key));
    id
  }

  /// Returns a stored capture and marks it used, or `None` once it was
  /// evicted, expired, or never produced by this Runner. A packed capture is
  /// unpacked.
  pub(super) fn get(&self, id: &str) -> Option<Arc<auv_driver::Capture>> {
    self.get_at(id, Instant::now())
  }

  fn get_at(&self, id: &str, now: Instant) -> Option<Arc<auv_driver::Capture>> {
    let mut inner = self.inner.lock().expect("capture store lock");
    inner.expire(now, self.options.idle);
    let key = inner.touch(id, now)?;
    let capture = inner.unpack(&key)?;
    inner.fit(self.options.budget_bytes, Some(key));
    Some(capture)
  }

  /// A cached OCR result for this capture, marking it used.
  pub(super) fn recognition(&self, id: &str, key: &RecognitionKey) -> Option<Arc<auv_driver::TextRecognition>> {
    let mut inner = self.inner.lock().expect("capture store lock");
    let blob = inner.touch(id, Instant::now())?;
    inner.blobs.get(&blob)?.recognitions.get(key).cloned()
  }

  /// Caches an OCR result on this capture's pixels.
  pub(super) fn remember_recognition(&self, id: &str, key: RecognitionKey, recognition: auv_driver::TextRecognition) {
    self.remember(id, |blob| {
      blob.recognitions.insert(key, Arc::new(recognition));
    });
  }

  /// A cached fetched image for this capture, marking it used.
  pub(super) fn image(&self, id: &str, key: &ImageKey) -> Option<Arc<EncodedImage>> {
    let mut inner = self.inner.lock().expect("capture store lock");
    let blob = inner.touch(id, Instant::now())?;
    inner.blobs.get(&blob)?.images.get(key).cloned()
  }

  /// Caches a fetched image on this capture's pixels.
  pub(super) fn remember_image(&self, id: &str, key: ImageKey, image: EncodedImage) {
    self.remember(id, |blob| {
      blob.images.insert(key, Arc::new(image));
    });
  }

  fn remember(&self, id: &str, store: impl FnOnce(&mut Blob)) {
    let mut inner = self.inner.lock().expect("capture store lock");
    let Some(key) = inner.entries.get(id).map(|entry| entry.blob) else {
      return;
    };
    let Some(blob) = inner.blobs.get_mut(&key) else {
      return;
    };
    let before = blob.derived_bytes();
    store(blob);
    let after = blob.derived_bytes();
    blob.bytes = blob.bytes + after - before;
    inner.total_bytes = inner.total_bytes + after - before;
    inner.fit(self.options.budget_bytes, Some(key));
  }

  /// Drops captures idle longer than the expiry and packs pixels idle longer
  /// than `cold_after`; called periodically so an unused Runner also gives
  /// its memory back.
  pub(super) fn sweep(&self) {
    self.sweep_at(Instant::now());
  }

  fn sweep_at(&self, now: Instant) {
    let mut inner = self.inner.lock().expect("capture store lock");
    inner.expire(now, self.options.idle);
    let cold: Vec<BlobKey> = inner
      .blobs
      .iter()
      .filter(|(_, blob)| matches!(blob.pixels, Pixels::Hot(_)) && now.saturating_duration_since(blob.last_used) > self.options.cold_after)
      .map(|(key, _)| *key)
      .collect();
    for key in cold {
      inner.pack(&key);
    }
  }

  #[cfg(test)]
  fn total_bytes(&self) -> usize {
    self.inner.lock().expect("capture store lock").total_bytes
  }

  #[cfg(test)]
  fn is_packed(&self, id: &str) -> bool {
    let inner = self.inner.lock().expect("capture store lock");
    let key = inner.entries[id].blob;
    matches!(inner.blobs[&key].pixels, Pixels::Cold { .. })
  }
}

/// A content hash over pixels and every metadata field, so equal keys mean
/// interchangeable captures.
fn content_key(capture: &auv_driver::Capture) -> BlobKey {
  let mut hasher = blake3::Hasher::new();
  hasher.update(&capture.image.width().to_le_bytes());
  hasher.update(&capture.image.height().to_le_bytes());
  hasher.update(
    format!("{:?}|{:?}|{}|{}|{:?}", capture.origin, capture.bounds, capture.scale_factor, capture.backend, capture.fallback_reason)
      .as_bytes(),
  );
  hasher.update(capture.image.as_raw());
  *hasher.finalize().as_bytes()
}

impl Inner {
  /// Marks an entry and its blob used; returns the blob key.
  fn touch(&mut self, id: &str, now: Instant) -> Option<BlobKey> {
    let entry = self.entries.get_mut(id)?;
    entry.last_used = now;
    let key = entry.blob;
    if let Some(blob) = self.blobs.get_mut(&key) {
      blob.last_used = now;
    }
    Some(key)
  }

  fn expire(&mut self, now: Instant, idle: Duration) {
    let expired: Vec<String> =
      self.entries.iter().filter(|(_, entry)| now.saturating_duration_since(entry.last_used) > idle).map(|(id, _)| id.clone()).collect();
    for id in expired {
      self.remove(&id);
    }
  }

  /// Gives memory back until the store fits `budget`: derived caches first,
  /// then packing hot pixels, then evicting captures, each least recently
  /// used first. `keep` (the blob just used) is evicted last.
  fn fit(&mut self, budget: usize, keep: Option<BlobKey>) {
    while self.total_bytes > budget {
      let by_age = |blobs: &HashMap<BlobKey, Blob>, wanted: &dyn Fn(&Blob) -> bool| {
        blobs.iter().filter(|(key, blob)| Some(**key) != keep && wanted(blob)).min_by_key(|(_, blob)| blob.last_used).map(|(key, _)| *key)
      };
      if let Some(key) = by_age(&self.blobs, &|blob| blob.derived_bytes() > 0) {
        let blob = self.blobs.get_mut(&key).expect("blob exists");
        let derived = blob.derived_bytes();
        blob.recognitions.clear();
        blob.images.clear();
        blob.bytes -= derived;
        self.total_bytes -= derived;
        continue;
      }
      if let Some(key) = by_age(&self.blobs, &|blob| matches!(blob.pixels, Pixels::Hot(_)))
        && self.pack(&key)
      {
        continue;
      }
      let oldest =
        self.entries.iter().filter(|(_, entry)| Some(entry.blob) != keep).min_by_key(|(_, entry)| entry.last_used).map(|(id, _)| id.clone());
      match oldest {
        Some(id) => self.remove(&id),
        None => break,
      }
    }
  }

  /// Packs a blob's pixels as QOI; false when it is cold already, packing
  /// does not save memory, or encoding fails.
  fn pack(&mut self, key: &BlobKey) -> bool {
    let Some(blob) = self.blobs.get_mut(key) else {
      return false;
    };
    let Pixels::Hot(capture) = &blob.pixels else {
      return false;
    };
    let (width, height) = capture.image.dimensions();
    let mut qoi = Vec::new();
    let encoded =
      image::codecs::qoi::QoiEncoder::new(&mut qoi).write_image(capture.image.as_raw(), width, height, image::ExtendedColorType::Rgba8);
    if encoded.is_err() || qoi.len() >= capture.image.as_raw().len() {
      return false;
    }
    let meta = auv_driver::Capture {
      image: image::RgbaImage::new(0, 0),
      ..auv_driver::Capture::clone(capture)
    };
    let saved = capture.image.as_raw().len() - qoi.len();
    blob.pixels = Pixels::Cold { meta, qoi };
    blob.bytes -= saved;
    self.total_bytes -= saved;
    true
  }

  /// The blob's capture, unpacking cold pixels back to hot.
  fn unpack(&mut self, key: &BlobKey) -> Option<Arc<auv_driver::Capture>> {
    let blob = self.blobs.get_mut(key)?;
    let capture = match &blob.pixels {
      Pixels::Hot(capture) => return Some(Arc::clone(capture)),
      Pixels::Cold { meta, qoi } => {
        let image = image::load_from_memory_with_format(qoi, image::ImageFormat::Qoi).ok()?.into_rgba8();
        Arc::new(auv_driver::Capture {
          image,
          ..meta.clone()
        })
      }
    };
    let packed = blob.pixels.bytes();
    blob.pixels = Pixels::Hot(Arc::clone(&capture));
    let unpacked = blob.pixels.bytes();
    blob.bytes = blob.bytes - packed + unpacked;
    self.total_bytes = self.total_bytes - packed + unpacked;
    Some(capture)
  }

  fn remove(&mut self, id: &str) {
    let Some(entry) = self.entries.remove(id) else {
      return;
    };
    let Some(blob) = self.blobs.get_mut(&entry.blob) else {
      return;
    };
    blob.holders -= 1;
    if blob.holders == 0 {
      let bytes = blob.bytes;
      self.blobs.remove(&entry.blob);
      self.total_bytes -= bytes;
    }
  }
}

#[cfg(test)]
#[path = "capture_store_test.rs"]
mod tests;
