use super::errors::BadPhoto;
use super::value_objects::{MAX_PHOTO_BYTES, Photo, PhotoKind};

fn a_png() -> Vec<u8> {
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    bytes.extend_from_slice(b"and then whatever an encoder wrote");
    bytes
}

fn a_jpeg() -> Vec<u8> {
    let mut bytes = vec![0xff, 0xd8, 0xff, 0xe0];
    bytes.extend_from_slice(b"JFIF and the rest");
    bytes
}

fn a_webp() -> Vec<u8> {
    let mut bytes = b"RIFF".to_vec();
    bytes.extend_from_slice(&[0x20, 0x00, 0x00, 0x00]);
    bytes.extend_from_slice(b"WEBPVP8 and the rest");
    bytes
}

#[test]
fn each_kind_is_recognised_by_its_first_bytes() {
    assert_eq!(
        Photo::examined(a_png()).expect("a png").kind(),
        PhotoKind::Png
    );
    assert_eq!(
        Photo::examined(a_jpeg()).expect("a jpeg").kind(),
        PhotoKind::Jpeg
    );
    assert_eq!(
        Photo::examined(a_webp()).expect("a webp").kind(),
        PhotoKind::Webp
    );
}

#[test]
fn anything_that_is_not_an_image_is_refused_however_it_is_announced() {
    assert_eq!(
        Photo::examined(b"<script>alert(1)</script>".to_vec()),
        Err(BadPhoto::NotAnImage)
    );
    assert_eq!(Photo::examined(Vec::new()), Err(BadPhoto::NotAnImage));
    assert_eq!(
        Photo::examined(b"RIFFxxxxWAVE".to_vec()),
        Err(BadPhoto::NotAnImage),
        "a riff container is not a webp unless it says so"
    );
}

#[test]
fn a_photo_past_the_ceiling_is_refused_before_it_is_sniffed() {
    let mut huge = a_png();
    huge.resize(MAX_PHOTO_BYTES + 1, 0);

    assert_eq!(
        Photo::examined(huge),
        Err(BadPhoto::TooLarge(MAX_PHOTO_BYTES)),
        "the size is the answer, rather than what a film is not"
    );
}

#[test]
fn a_photo_right_on_the_ceiling_is_kept() {
    let mut full = a_png();
    full.resize(MAX_PHOTO_BYTES, 0);

    assert!(Photo::examined(full).is_ok());
}

#[test]
fn a_photo_never_renders_its_bytes() {
    let rendered = format!("{:?}", Photo::examined(a_png()).expect("a png"));

    assert_eq!(rendered, "Photo { kind: Png, bytes: 42 }");
}

#[test]
fn a_kind_is_read_back_from_the_spelling_it_is_stored_under() {
    for kind in [PhotoKind::Png, PhotoKind::Jpeg, PhotoKind::Webp] {
        assert_eq!(PhotoKind::named(kind.as_str()), Some(kind));
    }
    assert_eq!(PhotoKind::named("image/gif"), None);
    assert_eq!(PhotoKind::named("png"), None);
}
