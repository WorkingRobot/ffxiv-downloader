pub mod chain;
pub mod graphviz;

use anyhow::{Result, anyhow};
use graphql_client::{GraphQLQuery, QueryBody, Response};
use reqwest::Client;
use serde::{Serialize, de::DeserializeOwned};

use crate::file::version::GameVersion;
use crate::thaliak::chain::{Node, Patch};
use crate::thaliak::{
    get_all_repositories::GetAllRepositoriesRepositories,
    get_repository_metadata::GetRepositoryMetadataRepository,
};

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/thaliak/2022-08-14.json",
    query_path = "src/thaliak/queries.graphql"
)]
struct GetRepositoryMetadata;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/thaliak/2022-08-14.json",
    query_path = "src/thaliak/queries.graphql"
)]
struct GetPatchChain;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/thaliak/2022-08-14.json",
    query_path = "src/thaliak/queries.graphql"
)]
struct GetAllRepositories;

const BASE_URL: &str = "https://thaliak.xiv.dev/graphql/2022-08-14";

async fn post<V: Serialize, D: DeserializeOwned>(
    client: &Client,
    body: &QueryBody<V>,
) -> Result<D> {
    let response: Response<D> = client
        .post(BASE_URL)
        .json(body)
        .send()
        .await?
        .json()
        .await?;

    match (response.data, response.errors) {
        (Some(data), _) => Ok(data),
        (None, Some(errors)) => Err(anyhow!("GraphQL errors: {errors:?}")),
        (None, None) => Err(anyhow!("No data or errors returned from GraphQL query")),
    }
}

pub async fn get_repository_metadata(
    client: &Client,
    slug: impl Into<String>,
) -> Result<GetRepositoryMetadataRepository> {
    let body = GetRepositoryMetadata::build_query(get_repository_metadata::Variables {
        repo_id: slug.into(),
    });
    post::<_, get_repository_metadata::ResponseData>(client, &body)
        .await?
        .repository
        .ok_or_else(|| anyhow!("No such repository"))
}

pub async fn get_all_repositories(client: &Client) -> Result<Vec<GetAllRepositoriesRepositories>> {
    let body = GetAllRepositories::build_query(get_all_repositories::Variables {});
    Ok(post::<_, get_all_repositories::ResponseData>(client, &body)
        .await?
        .repositories)
}

async fn query_versions(client: &Client, slug: &str) -> Result<Vec<Node>> {
    let body = GetPatchChain::build_query(get_patch_chain::Variables {
        repo_id: slug.to_string(),
    });
    let repository = post::<_, get_patch_chain::ResponseData>(client, &body)
        .await?
        .repository
        .ok_or_else(|| anyhow!("No such repository: {slug}"))?;

    repository
        .versions
        .into_iter()
        .map(|version| {
            Ok(Node {
                version: GameVersion::new(&version.version_string)?,
                is_active: version.is_active,
                prerequisites: version
                    .prerequisite_versions
                    .into_iter()
                    .map(|prereq| GameVersion::new(&prereq.version_string))
                    .collect::<Result<_>>()?,
                patches: version
                    .patches
                    .into_iter()
                    .map(|patch| Patch {
                        url: patch.url,
                        size: patch.size,
                    })
                    .collect(),
            })
        })
        .collect()
}
