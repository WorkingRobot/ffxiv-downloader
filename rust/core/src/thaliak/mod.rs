use anyhow::Result;
use graphql_client::{GraphQLQuery, Response};
use reqwest::Client;

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

// #[derive(GraphQLQuery)]
// #[graphql(
//     schema_path = "src/thaliak/2022-08-14.json",
//     query_path = "src/thaliak/queries.graphql"
// )]
// struct GetPatchChain;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/thaliak/2022-08-14.json",
    query_path = "src/thaliak/queries.graphql"
)]
struct GetAllRepositories;

const BASE_URL: &str = "https://thaliak.xiv.dev/graphql/2022-08-14";

pub async fn get_repository_metadata(
    client: &Client,
    slug: impl Into<String>,
) -> Result<GetRepositoryMetadataRepository> {
    let request_body = GetRepositoryMetadata::build_query(get_repository_metadata::Variables {
        repo_id: slug.into(),
    });

    let response: Response<get_repository_metadata::ResponseData> = client
        .post(BASE_URL)
        .json(&request_body)
        .send()
        .await?
        .json()
        .await?;
    if let Some(data) = response.data
        && let Some(repository) = data.repository
    {
        Ok(repository)
    } else if let Some(errors) = response.errors {
        Err(anyhow::anyhow!("GraphQL errors: {:?}", errors))
    } else {
        Err(anyhow::anyhow!(
            "No data or errors returned from GraphQL query"
        ))
    }
}

pub async fn get_all_repositories(client: &Client) -> Result<Vec<GetAllRepositoriesRepositories>> {
    let request_body = GetAllRepositories::build_query(get_all_repositories::Variables {});

    let response: Response<get_all_repositories::ResponseData> = client
        .post(BASE_URL)
        .json(&request_body)
        .send()
        .await?
        .json()
        .await?;

    if let Some(data) = response.data {
        Ok(data.repositories)
    } else if let Some(errors) = response.errors {
        Err(anyhow::anyhow!("GraphQL errors: {:?}", errors))
    } else {
        Err(anyhow::anyhow!(
            "No data or errors returned from GraphQL query"
        ))
    }
}
