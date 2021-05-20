use std::{
    cell::RefCell,
    collections::HashMap,
    io::{stdin, stdout, Write},
    rc::Rc,
    str::FromStr,
};

use anyhow::Context;
use canvas_gc::{build_client, read_config};
use graphql_client::{GraphQLQuery, Response};

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "gql/schema.json",
    query_path = "gql/assignment_groups.gql"
)]
pub struct AssignmentGroupsQuery;

#[allow(dead_code)]
pub struct AssignmentGroup {
    id: String,
    name: String,
    grading_weight: f64,
}

#[derive(GraphQLQuery)]
#[graphql(schema_path = "gql/schema.json", query_path = "gql/assignments.gql")]
pub struct AssignmentsQuery;

#[derive(Clone)]
pub struct Assignment {
    id: String,
    name: String,
    ag_id: String,
    max_points: f64,
    points: f64,
}

pub struct Assignment2 {
    id: String,
    name: String,
    ag_id: String,
    max_points: f64,
    points: Option<f64>,
}

fn grade(
    ags: &HashMap<String, AssignmentGroup>,
    assignments_by_agid: &HashMap<String, Vec<Rc<RefCell<Assignment>>>>,
) -> f64 {
    ags.values()
        .map(|ag| {
            let (a1, a2) = if let Some(a3) = assignments_by_agid.get(&ag.id) {
                a3.iter().fold((0., 0.), |acc, a| {
                    (acc.0 + a.borrow().points, acc.1 + a.borrow().max_points)
                })
            } else {
                (1., 1.)
            };
            (ag.grading_weight, a1 / a2)
        })
        .fold(0., |acc, (gw, v)| acc + gw * v)
}

fn mergify<'a>(
    a1: &'a HashMap<String, Vec<Rc<RefCell<Assignment>>>>,
    a2: &'a HashMap<String, Vec<Rc<RefCell<Assignment2>>>>,
) -> HashMap<String, Vec<Rc<RefCell<Assignment>>>> {
    let mut out = HashMap::<String, Vec<Rc<RefCell<Assignment>>>>::new();
    for (x2, y2) in a2 {
        for y in y2 {
            let yr = y.borrow();
            if yr.points.is_some() {
                let u = out.entry(x2.clone()).or_default();
                u.push(Rc::new(RefCell::new(Assignment {
                    ag_id: yr.ag_id.clone(),
                    id: yr.id.clone(),
                    max_points: yr.max_points,
                    name: yr.name.clone(),
                    points: yr.points.unwrap(),
                })));
            }
        }
    }
    // n.b. doing this after so that overrides are taken first
    for (x1, y1) in a1 {
        let u = out.entry(x1.clone()).or_default();
        for y in y1 {
            // FIXME: doesn't work? overrides aren't honored
            if u.iter().find(|&z| z.borrow().id == y.borrow().id).is_none() {
                u.push(y.clone());
            }
        }
        // u.extend(y1.iter().cloned());
        // u.sort_by_key(|r| r.borrow().id.clone());
        // u.dedup_by_key(|r| r.borrow().id.clone());
    }
    out
}

fn gimme<T: FromStr>(s: &str) -> T {
    fn gimme_inner<T: FromStr>(s: &str) -> Option<T> {
        print!("{}", s);
        stdout().flush().unwrap();
        let mut v = String::new();
        stdin().read_line(&mut v).unwrap();
        v.trim().parse().ok()
    }
    loop {
        let x = gimme_inner(s);
        if let Some(y) = x {
            return y;
        }
    }
}

fn gimme_check<T: FromStr, F: Fn(&T) -> bool>(s: &str, f: F) -> T {
    loop {
        let x = gimme(s);
        if f(&x) {
            return x;
        }
    }
}

pub enum GradeOrRemove {
    Remove,
    Grade(f64),
}

impl FromStr for GradeOrRemove {
    type Err = <f64 as FromStr>::Err;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "-" {
            Ok(Self::Remove)
        } else {
            s.parse().map(Self::Grade)
        }
    }
}

fn main() -> anyhow::Result<()> {
    let config = read_config();
    let cid = config.cid.expect("Need a CID, check lib.rs for details");

    let client = build_client(&config.token)?;

    // Fetch assignment groups
    let agq = AssignmentGroupsQuery::build_query(assignment_groups_query::Variables {
        cid: Some(cid.clone()),
    });
    let agq_res = client.post(&config.api).json(&agq).send()?;
    agq_res.error_for_status_ref()?;
    let agq_res = agq_res
        .json::<Response<assignment_groups_query::ResponseData>>()?
        .data
        .context("assignment groups response")?;
    let mut ags = HashMap::new();

    for ag in agq_res
        .course
        .unwrap()
        .assignment_groups_connection
        .unwrap()
        .nodes
        .unwrap()
    {
        for node in ag {
            // let node = node.unwrap();
            let (id, name, gw) = (
                node.id,
                node.name.unwrap(),
                node.group_weight.unwrap() / 100.,
            );
            println!("assignment group id={} name={} weight={}%", id, name, gw,);
            ags.insert(
                id.clone(),
                AssignmentGroup {
                    id,
                    name,
                    grading_weight: gw,
                },
            );
        }
    }

    // Fetch assignments and start grouping
    let aq = AssignmentsQuery::build_query(assignments_query::Variables { cid: Some(cid) });
    let aq_res = client.post(&config.api).json(&aq).send()?;
    aq_res.error_for_status_ref()?;
    let aq_res = aq_res
        .json::<Response<assignments_query::ResponseData>>()?
        .data
        .context("assignment groups response")?;
    let mut assignments = HashMap::new();
    let mut assignments2 = HashMap::new();
    let mut assignments_by_agid = HashMap::<String, Vec<Rc<RefCell<Assignment>>>>::new();
    let mut assignments2_by_agid = HashMap::<String, Vec<Rc<RefCell<Assignment2>>>>::new();

    for assignment in aq_res
        .course
        .unwrap()
        .assignments_connection
        .unwrap()
        .nodes
        .unwrap()
    {
        let assignment = assignment.unwrap();
        let mut points = None;
        let (id, name, ag_id, max_points) = (
            assignment.id,
            assignment.name.unwrap(),
            assignment.assignment_group.unwrap().id,
            assignment.points_possible.unwrap(),
        );
        for submission in assignment.submissions_connection.unwrap().nodes.unwrap() {
            let submission = submission.unwrap();
            if let Some(s) = submission.score {
                points = Some(s);
            }
        }
        println!(
            "assignment id={} name={} agid={} points={}/{}",
            id,
            name,
            ag_id,
            points.unwrap_or(0.),
            max_points
        );
        if points.is_none() {
            assignments2.insert(
                id.clone(),
                Rc::new(RefCell::new(Assignment2 {
                    id: id.clone(),
                    name: name.clone(),
                    ag_id: ag_id.clone(),
                    points,
                    max_points,
                })),
            );
            continue;
        }
        assignments.insert(
            id.clone(),
            Rc::new(RefCell::new(Assignment {
                id,
                name,
                ag_id,
                points: points.unwrap(),
                max_points,
            })),
        );
    }

    for assignment in assignments.values() {
        assignments_by_agid
            .entry(assignment.borrow().ag_id.clone())
            .or_default()
            .push(assignment.clone());
    }

    for assignment2 in assignments2.values() {
        assignments2_by_agid
            .entry(assignment2.borrow().ag_id.clone())
            .or_default()
            .push(assignment2.clone());
    }

    let grade1 = grade(&ags, &assignments_by_agid);
    println!("Graded value = {}%", grade1 * 100.);

    loop {
        let r = gimme("Type an option - 1 for modify assignment, 2 for grades, 3 to exit\n> ");
        match r {
            1 => {
                for a in assignments.values() {
                    let a = a.borrow();
                    println!(
                        "assignment id={} name={} points={}/{}",
                        a.id, a.name, a.points, a.max_points
                    );
                }
                for a2 in assignments2.values() {
                    let a2 = a2.borrow();
                    println!(
                        "(what-if) assignment id={} name={} graded={} points={}/{}",
                        a2.id,
                        a2.name,
                        a2.points.is_some(),
                        a2.points.unwrap_or(0.),
                        a2.max_points
                    );
                }
                let aid = gimme_check::<String, _>("Give me an ID to modify: ", |id| {
                    assignments.contains_key(id) || assignments2.contains_key(id)
                });
                let aref = assignments2.entry(aid.clone()).or_insert_with(|| {
                    let a = assignments[&aid].borrow();
                    Rc::new(RefCell::new(Assignment2 {
                        id: a.id.clone(),
                        ag_id: a.ag_id.clone(),
                        max_points: a.max_points,
                        name: a.name.clone(),
                        points: Some(a.points),
                    }))
                });
                let mut aref2 = aref.borrow_mut();
                let grade = gimme_check::<GradeOrRemove, _>(
                    "Give me a new grade or - to remove a grade: ",
                    |g| {
                        if let GradeOrRemove::Grade(g) = g {
                            *g <= aref2.max_points
                        } else {
                            true
                        }
                    },
                );
                match grade {
                    GradeOrRemove::Remove => {
                        if assignments.contains_key(&aid) {
                            drop(aref2);
                            assignments2.remove(&aid);
                        } else {
                            aref2.points = None;
                        }
                    }
                    GradeOrRemove::Grade(g) => aref2.points = Some(g),
                }
                println!("Updated!");
            }
            2 => {
                let ax = mergify(&assignments_by_agid, &assignments2_by_agid);
                let g = grade(&ags, &ax);
                println!("Grade = {}%", g * 100.);
            }
            3 => break,
            _ => {}
        }
    }

    println!("bye");

    Ok(())
}
