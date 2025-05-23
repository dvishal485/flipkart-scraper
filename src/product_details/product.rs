use crate::product_details::{Offer, Seller, Specification, Specifications};
use scraper::{Html, Selector};
use url::Url;

#[cfg(feature = "fetch")]
use super::errors::FetchError;
#[cfg(feature = "fetch")]
use reqwest::Client;

#[cfg(feature = "wasm_parser")]
use tsify::Tsify;
#[cfg(feature = "wasm_parser")]
use wasm_bindgen::prelude::*;

use super::errors::ProductDetailsError;

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "wasm_parser", derive(Tsify), tsify(into_wasm_abi))]
#[derive(Default, Debug)]
/// The details of a Flipkart Product.
///
/// Use the `ProductDetails::fetch` method to fetch the details of a product
/// from the product url.
pub struct ProductDetails {
    #[cfg_attr(feature = "wasm_parser", tsify(optional))]
    /// Product name
    pub name: Option<String>,
    /// Whether the product is in stock or not.
    pub in_stock: bool,
    #[cfg_attr(feature = "wasm_parser", tsify(optional))]
    /// Current price of the product.
    pub current_price: Option<i32>,
    #[cfg_attr(feature = "wasm_parser", tsify(optional))]
    /// Original price of the product.
    pub original_price: Option<i32>,
    #[cfg_attr(feature = "wasm_parser", tsify(optional))]
    /// Product ID
    pub product_id: Option<String>,
    /// URL to product, usually shortened and cleaner.
    pub share_url: String,
    #[cfg_attr(feature = "wasm_parser", tsify(optional))]
    /// Rating of the product.
    pub rating: Option<f32>,
    /// Whether it is f-assured produtc or not.
    pub f_assured: bool,
    /// Highlights of the product.
    pub highlights: Vec<String>,
    #[cfg_attr(feature = "wasm_parser", tsify(optional))]
    /// Primary seller of the product.
    pub seller: Option<Seller>,
    /// URL to thumbnails of the product.
    pub thumbnails: Vec<String>,
    /// Offers available on the product.
    pub offers: Vec<Offer>,
    /// Specifications of the product.
    pub specifications: Vec<Specifications>,
}

impl ProductDetails {
    /// Parse webpage content and return the product details.
    pub fn parse(webpage_content: String) -> Result<Self, ProductDetailsError> {
        let div_selector = &Selector::parse("div").unwrap();
        let h1_selector = &Selector::parse("h1").unwrap();
        let title_selector = &Selector::parse("title").unwrap();
        let script_selector = &Selector::parse("script").unwrap();
        let img_selector = &Selector::parse("img").unwrap();
        let li_selector = &Selector::parse("li").unwrap();
        let ul_selector = &Selector::parse("ul").unwrap();
        let seller_selector = &Selector::parse("#sellerName").unwrap();
        let span_selector = &Selector::parse("span").unwrap();
        let table_selector = &Selector::parse("table").unwrap();
        let tr_selector = &Selector::parse("tr").unwrap();
        let td_selector = &Selector::parse("td").unwrap();

        if webpage_content.contains("has been moved or deleted")
            || webpage_content.contains("not right!")
        {
            return Err(ProductDetailsError::NonProductLink);
        }
        if webpage_content.contains("Internal Server Error") {
            return Err(ProductDetailsError::InternalServerError);
        }
        let document = Html::parse_document(&webpage_content);

        let retry_error = webpage_content.contains("Retry in ");
        if retry_error {
            return Err(ProductDetailsError::FlipkartRetryError);
        }

        let mut details = ProductDetails::default();

        let title = document
            .select(h1_selector)
            .next()
            .or_else(|| document.select(title_selector).next())
            .map(|title| title.text().collect::<String>());

        if let Some(title) = &title {
            if title == "Are you a human?" {
                return Err(ProductDetailsError::IdentifiedAsBot);
            }
        }

        details.name = title;

        // thumbnails
        let unordered_lists = document.select(ul_selector);
        for list in unordered_lists {
            if !list.text().collect::<String>().trim().is_empty() {
                continue;
            }
            let thumbnails = &mut details.thumbnails;
            for list_item in list.select(li_selector) {
                for image in list_item.select(img_selector) {
                    if let Some(src) = image.value().attr("src") {
                        thumbnails.push(src.into());
                    }
                }
            }
            if !thumbnails.is_empty() {
                break;
            }
        }

        let coming_soon = webpage_content.contains("Coming Soon");
        let in_stock = !(coming_soon || webpage_content.contains("currently out of stock"));
        details.in_stock = in_stock;

        if in_stock {
            let seller = document
                .select(seller_selector)
                .next()
                .map(|seller_elem| {
                    (
                        seller_elem.select(span_selector).next(),
                        seller_elem.select(div_selector).next(),
                    )
                })
                .and_then(|(span_elem, div_elem)| {
                    let name = span_elem
                        .and_then(|elem| elem.text().next().map(|t| t.to_string()))
                        .or_else(|| {
                            div_elem
                                .map(|elem| elem.text().collect::<String>())
                                .map(|name| name.trim().to_string())
                        });
                    if let Some(name) = name {
                        let rating = div_elem
                            .map(|elem| elem.text().collect::<String>())
                            .and_then(|rating| rating.trim().parse::<f32>().ok());
                        Some(Seller { name, rating })
                    } else {
                        None
                    }
                });
            details.seller = seller;
        }

        let star_svg = include_str!("../star_base64_svg").trim();
        for element in document.select(div_selector) {
            let text = element.text().next().unwrap_or_default();
            let text = text.trim();

            if details.highlights.is_empty() && text.starts_with("Highlights") {
                if let Some(ul_elem) = element.select(ul_selector).next() {
                    let pointers = ul_elem.select(li_selector);
                    for pointer in pointers {
                        let text = pointer.text().collect::<String>();
                        details.highlights.push(text);
                    }
                }
            }

            if in_stock && text.starts_with("Available offers") {
                for offer in element.select(li_selector) {
                    let offer_container = offer.select(span_selector).next();
                    let mut category = offer_container.map(|e| e.text().collect::<String>());
                    let description =
                        offer_container
                            .and_then(|e| e.next_sibling())
                            .and_then(|e| {
                                if e.value().as_element().map(|e| e.name()) == Some("span") {
                                    e.first_child()
                                        .and_then(|t| t.value().as_text().map(|t| t.to_string()))
                                } else {
                                    category.take()
                                }
                            });

                    if let Some(description) = description {
                        details.offers.push(Offer {
                            category,
                            description,
                        });
                    }
                }
            }

            if details.specifications.is_empty() && text.starts_with("Specifications") {
                details.specifications = element
                    .select(table_selector)
                    .filter_map(|table| {
                        table.prev_sibling().and_then(|elem| {
                            if let Some(category) = elem.first_child() {
                                let category = category.value().as_text().map(|t| t.to_string())?;
                                let x = table
                                    .select(tr_selector)
                                    .filter_map(|row| {
                                        let mut td = row.select(td_selector);
                                        let key = td.next().map(|t| t.text().collect::<String>());
                                        let val = td.next().map(|t| t.text().collect::<String>());
                                        if let (Some(key), Some(val)) = (key, val) {
                                            Some(Specification {
                                                name: key,
                                                value: val,
                                            })
                                        } else {
                                            None
                                        }
                                    })
                                    .collect();
                                Some(Specifications {
                                    category,
                                    specifications: x,
                                })
                            } else {
                                None
                            }
                        })
                    })
                    .collect();
            }

            if coming_soon {
                // product won't contain price or rating
                continue;
            }

            if details.rating.is_none() {
                if let Some(img_elem) = element.select(img_selector).next() {
                    if let Some(img_src) = img_elem.value().attr("src") {
                        if img_src.trim() == star_svg {
                            details.rating = text.parse::<f32>().ok();
                        }
                    }
                }
            }

            if details.current_price.is_none() {
                // test for f-assured product comes before price is set
                for img in element.select(img_selector) {
                    if let Some(img_src) = img.value().attr("src") {
                        if img_src.contains("fa_62673a.png") {
                            details.f_assured = true;
                            continue;
                        }
                    }
                }
            }

            if details.original_price.is_none() && text.starts_with('₹') {
                for elem in element.select(div_selector) {
                    let text = elem.text().collect::<String>();
                    let text = text.strip_prefix('₹').unwrap();
                    if text.contains('₹') {
                        continue;
                    }
                    let price_tag = text.replace(',', "").parse::<i32>().ok();
                    if details.current_price.is_none() {
                        details.current_price = price_tag;
                    } else {
                        details.original_price = price_tag.or(details.current_price);
                        break;
                    }
                }
            }
        }

        'link_identifier: for element in document.select(script_selector) {
            let text = element.text().collect::<String>();
            if text.starts_with("window.__INITIAL_STATE__") {
                if let Some((_, id_container)) = text.split_once("productId") {
                    let pattern: &[_] = &['"', ':'];
                    let id_container = id_container.trim().trim_matches(pattern);
                    details.product_id = id_container.split_once('"').map(|(id, _)| id.into());
                }
                for content in text.split_inclusive("product.share.pp") {
                    if let Some(link_to_product) = content.rsplit_once('"') {
                        // try parse url
                        if let Ok(link) = Url::parse(link_to_product.1) {
                            details.share_url = link.into();
                            break 'link_identifier;
                        }
                    }
                }
            }
        }

        Ok(details)
    }

    #[cfg(feature = "fetch")]
    /// Fetches a product from the given url.
    ///
    /// ```rust
    /// use std::error::Error;
    /// use flipkart_scraper::{ProductDetails, Url};
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn Error>> {
    ///     let url = "https://www.flipkart.com/samsung-galaxy-f13-waterfall-blue-64-gb/p/itm583ef432b2b0c";
    ///     let details = ProductDetails::fetch(Url::parse(url)?).await;
    ///     println!("{:#?}", details);
    ///     Ok(())
    /// }
    /// ```
    pub async fn fetch(url: Url) -> Result<Self, FetchError> {
        if !url
            .domain()
            .ok_or(FetchError::InvalidDomainName)?
            .contains("flipkart.com")
        {
            return Err(FetchError::NonFlipkartDomain);
        }

        let client = Client::builder()
            .default_headers(crate::build_headers())
            .build()
            .map_err(|source| FetchError::ClientBuilderError { source })?;

        let webpage = client
            .get(url.to_owned())
            .send()
            .await
            .map_err(|source| FetchError::HttpRequestError { source })?;
        let body = webpage
            .text()
            .await
            .map_err(|source| FetchError::WebpageTextParseError { source })?;

        let details = Self::parse(body).and_then(|mut p| {
            if p.share_url.is_empty() {
                p.share_url = url.into();
            }
            Ok(p)
        });

        details.map_err(|source| FetchError::ProductDetailsError { source })
    }
}
