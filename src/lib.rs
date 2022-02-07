//! # Rocket Validation
//!
//! Welcome to the Rocket Validation crate. If you are looking to validate your Json, Form or Query Structs using Rocket you have come to the right place!
//!
//! ## Why
//! Rocket is using Rusts powerful typing system. Which is amazing because you can be sure its what you want. But is it? How about kebab-case strings or phone number inputs, these aren’t really types.
//! You could implement a [custom deserializer](https://docs.serde.rs/serde/de/trait.Deserializer.html) for a wrapped type or write custom logic to validate it on endpoint calls, thats error prone and not ergonomic and doesn't allow you to return meaningful and contextual errors.
//!
//! If you are coming from TypeScript you might have heard of [class-validator](https://github.com/typestack/class-validator) which is simple, declarative and can be implemented into middleware. Using [validator](https://github.com/Keats/validator) this crate achieves a similar result using rockets [guard](https://rocket.rs/v0.5-rc/guide/requests/#request-guards) mechanism.
//! > Anything implementing [FromData](https://api.rocket.rs/v0.5-rc/rocket/data/trait.FromData.html), [FromRequest](https://api.rocket.rs/v0.5-rc/rocket/request/trait.FromRequest.html) or [FromForm](https://api.rocket.rs/v0.5-rc/rocket/form/trait.FromForm.html) as well as [`Validate`](https://docs.rs/validator/latest/validator/#example) are able to use the `Validated` guard of this crate, so you can be sure your data is validated once you receive it in your handler. (Including rockets [`Json`](https://rocket.rs/v0.5-rc/guide/requests/#json) type)
//!
//! > Using rockets [catchers](https://rocket.rs/v0.5-rc/guide/requests/#error-catchers) you are able to route errors which occurs during validation to your user.
//!
//! Current validation in rocket: Rocket has validation for FromForm structs but for nothing else.
//!
//! ## Usage
//!
//! In order to get going, you need to depend on the `rocket-validation`.
//!
//! Add this to your `Cargo.toml`
//! ```toml
//! [dependencies]
//! rocket-validation = "0.1.0"
//! ```
//!
//! Now you can go on and implement your Validation
//! ```rust
//! // Because we use rocket....
//! #[macro_use]
//! extern crate rocket;
//!
//! // Some types for Json types
//! use rocket::serde::{json::Json, Deserialize, Serialize};
//!
//! // Will be important for validation....
//! use rocket_validation::{Validate, Validated};
//!
//! #[derive(Debug, Deserialize, Serialize, Validate)] // Implements `Validate`
//! #[serde(crate = "rocket::serde")]
//! pub struct HelloData {
//!     #[validate(length(min = 1))] // Your validation annotation
//!     name: String,
//!     #[validate(range(min = 0, max = 100))] // Your validation annotation
//!     age: u8,
//! }
//!
//! #[post("/hello", format = "application/json", data = "<data>")]
//! fn validated_hello(
//!     data: /* Uses the `Validated` type */ Validated<Json<HelloData>>,
//! ) -> Json<HelloData> {
//!     Json(data.0 .0)
//! }
//!
//! #[launch]
//! fn rocket() -> _ {
//!     rocket::build().mount("/", routes![hello, validated_hello])
//! }
//! ```
//! ### Exposing errors to clients
//!
//! > Before you use the following, you should be aware of what errors you expose to your clients as well as what that means for security.
//!
//! If you would like to respond invalid requests with some custom messages, you can implement the `validation_catcher` catcher to do so.
//! ```rust
//! #[launch]
//! fn rocket() -> _ {
//!     rocket::build()
//!         .mount("/", routes![hello, validated_hello])
//!         .register("/", catchers![rocket_validation::validation_catcher])
//! }
//! ```
#![deny(clippy::all, clippy::cargo)]
#![forbid(unsafe_code)]

#[allow(unused_imports)]
#[macro_use]
pub extern crate validator;

#[macro_use]
extern crate rocket;

use rocket::{
    data::{Data, FromData, Outcome as DataOutcome},
    form,
    form::{DataField, FromForm, ValueField},
    http::Status,
    outcome::Outcome,
    request::{FromRequest, Request},
    serde::{json::Json, Serialize},
};
use std::fmt::Debug;
pub use validator::{Validate, ValidationErrors};

/// Struct used for Request Guards
#[derive(Clone, Debug)]
pub struct Validated<T>(pub T);

// Impl to get type T of `Json`
impl<T> Validated<Json<T>> {
    #[inline]
    pub fn into_deep_inner(self) -> T {
        self.0 .0
    }
}

// Impl to get type T
impl<T> Validated<T> {
    #[inline]
    pub fn into_inner(self) -> T {
        self.0
    }
}

// Struct representing errors sent by the catcher
#[derive(Serialize)]
#[serde(crate = "rocket::serde")]
pub struct Error<'a> {
    code: u128,
    message: &'a str,
    errors: Option<&'a ValidationErrors>,
}

// Catcher to return validation errors to the client
#[catch(400)]
pub fn validation_catcher<'a>(req: &'a Request) -> Json<Error<'a>> {
    Json(Error {
        code: 400,
        message: "Bad Request. The request could not be understood by the server due to malformed \
                  syntax.",
        errors: req.local_cache(|| CachedValidationErrors(None)).0.as_ref(),
    })
}

// Wrapper used to store `ValidationErrors` within the scope of the request
#[derive(Clone)]
pub struct CachedValidationErrors(pub Option<ValidationErrors>);

// Implementation of `Validated` for `FromData`
#[rocket::async_trait]
impl<'r, D: Validate + FromData<'r>> FromData<'r> for Validated<D> {
    type Error = Result<ValidationErrors, <D as rocket::data::FromData<'r>>::Error>;

    async fn from_data(req: &'r Request<'_>, data: Data<'r>) -> DataOutcome<'r, Self> {
        let data_outcome = <D as FromData<'r>>::from_data(req, data).await;

        match data_outcome {
            Outcome::Failure((status, err)) => Outcome::Failure((status, Err(err))),
            Outcome::Forward(err) => Outcome::Forward(err),
            Outcome::Success(data) => match data.validate() {
                Ok(_) => Outcome::Success(Validated(data)),
                Err(err) => {
                    req.local_cache(|| CachedValidationErrors(Some(err.to_owned())));
                    Outcome::Failure((Status::BadRequest, Ok(err)))
                }
            },
        }
    }
}

// Implementation of `Validated` for `FromRequest`
#[rocket::async_trait]
impl<'r, D: Validate + FromRequest<'r>> FromRequest<'r> for Validated<D> {
    type Error = Result<ValidationErrors, D::Error>;
    async fn from_request(req: &'r Request<'_>) -> rocket::request::Outcome<Self, Self::Error> {
        let data_outcome = D::from_request(req).await;

        match data_outcome {
            Outcome::Failure((status, err)) => Outcome::Failure((status, Err(err))),
            Outcome::Forward(err) => Outcome::Forward(err),
            Outcome::Success(data) => match data.validate() {
                Ok(_) => Outcome::Success(Validated(data)),
                Err(err) => {
                    req.local_cache(|| CachedValidationErrors(Some(err.to_owned())));
                    Outcome::Failure((Status::BadRequest, Ok(err)))
                }
            },
        }
    }
}

// Implementation of `Validated` for `FromForm`
#[rocket::async_trait]
impl<'r, T: Validate + FromForm<'r>> FromForm<'r> for Validated<T> {
    type Context = T::Context;

    #[inline]
    fn init(opts: form::Options) -> Self::Context {
        T::init(opts)
    }

    #[inline]
    fn push_value(ctxt: &mut Self::Context, field: ValueField<'r>) {
        T::push_value(ctxt, field)
    }

    #[inline]
    async fn push_data(ctxt: &mut Self::Context, field: DataField<'r, '_>) {
        T::push_data(ctxt, field).await
    }

    fn finalize(this: Self::Context) -> form::Result<'r, Self> {
        match T::finalize(this) {
            Err(err) => Err(err),
            Ok(data) => match data.validate() {
                Ok(_) => Ok(Validated(data)),
                Err(err) => Err(err
                    .into_errors()
                    .into_iter()
                    .map(|e| form::Error {
                        name: Some(e.0.into()),
                        kind: form::error::ErrorKind::Validation(std::borrow::Cow::Borrowed(e.0)),
                        value: None,
                        entity: form::error::Entity::Value,
                    })
                    .collect::<Vec<_>>()
                    .into()),
            },
        }
    }
}
