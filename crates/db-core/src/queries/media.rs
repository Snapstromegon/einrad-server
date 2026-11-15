//! Media queries: the competition's images and the sponsors.
//!
//! The read models carry no serialization; the handler maps them to its
//! response types.

use time::OffsetDateTime;

use crate::{actor::Actor, error::QueryError, media::Image};

/// A sponsor as the info board lists it: only sponsors with an image.
#[derive(Debug, Clone, PartialEq)]
pub struct Sponsor {
    pub id: i64,
    pub name: String,
    pub link: Option<String>,
    /// Changes with the image.
    pub updated_at: OffsetDateTime,
}

impl Sponsor {
    /// The listed sponsors of `images` (in their order): those with bytes.
    pub fn listed(images: Vec<Image>) -> Vec<Sponsor> {
        images
            .into_iter()
            .filter(|image| image.kind == "sponsor" && image.has_data)
            .map(|image| Sponsor {
                id: image.id,
                name: image.name,
                link: image.link,
                updated_at: image.updated_at,
            })
            .collect()
    }
}

/// The read side of the media area of the API; one method per endpoint.
#[async_trait::async_trait]
pub trait MediaQueries: Send + Sync {
    /// All images: logo, break image, then the sponsors in order (no bytes).
    async fn get_competition_images(&self, actor: &Actor) -> Result<Vec<Image>, QueryError>;

    /// Public: the sponsors with an image, in order.
    async fn list_sponsors(&self) -> Result<Vec<Sponsor>, QueryError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(id: i64, kind: &str, has_data: bool) -> Image {
        Image {
            id,
            kind: kind.into(),
            position: 0,
            name: format!("n{id}"),
            link: None,
            source_url: None,
            mime_type: None,
            has_data,
            updated_at: OffsetDateTime::UNIX_EPOCH,
            fetched_at: None,
            fetch_error: None,
        }
    }

    #[test]
    fn only_sponsors_with_an_image_are_listed() {
        let listed = Sponsor::listed(vec![
            image(1, "logo", true),
            image(2, "sponsor", false),
            image(3, "sponsor", true),
        ]);
        assert_eq!(listed.iter().map(|s| s.id).collect::<Vec<_>>(), vec![3]);
    }
}
