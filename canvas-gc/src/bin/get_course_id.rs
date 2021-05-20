use anyhow::Context;
use canvas_gc::{build_client, read_config};
use graphql_client::{GraphQLQuery, Response};

#[derive(GraphQLQuery)]
#[graphql(schema_path = "gql/schema.json", query_path = "gql/courses.gql")]
pub struct CoursesQuery;

fn main() -> anyhow::Result<()> {
    let config = read_config();
    let client = build_client(&config.token)?;

    // Fetch courses
    let cq = CoursesQuery::build_query(courses_query::Variables);
    let cq_res = client.post(&config.api).json(&cq).send()?;
    cq_res.error_for_status_ref()?;
    let cq_res = cq_res
        .json::<Response<courses_query::ResponseData>>()?
        .data
        .context("assignment groups response")?;

    for c in cq_res.all_courses {
        for c2 in c {
            println!("course id={} name={}", c2.id, c2.name);
        }
    }

    Ok(())
}
