//! API contract tests are kept in a separate module to keep `api.rs` readable.

#[cfg(test)]
mod tests {
    use super::super::api::{dispatch, AppState};
    use crate::http::Request;
    use ws_core::json::Json;

    fn request(method: &str, path: &str, body: &str) -> Request {
        Request {
            method: method.to_string(),
            path: path.to_string(),
            query_raw: String::new(),
            query: Vec::new(),
            headers: Vec::new(),
            body: body.as_bytes().to_vec(),
        }
    }

    fn body(response: crate::http::Response) -> Json {
        Json::parse(&String::from_utf8(response.body).unwrap()).unwrap()
    }

    #[test]
    fn synchronous_world_contract_is_atomic() {
        let state = AppState::new();
        let created = body(dispatch(
            &request(
                "POST",
                "/v1/worlds",
                r#"{"name":"contract","config":{"auto_start":false,"quality":"low","systems":{"fluid":{"enabled":false},"heat":{"enabled":false}}}}"#,
            ),
            &state,
        ));
        let id = created.get("id").unwrap().as_str().unwrap().to_string();

        let before = body(dispatch(
            &request("GET", &format!("/v1/worlds/{id}"), ""),
            &state,
        ));
        assert_eq!(before.get("tick").unwrap().as_u64(), Some(0));
        assert_eq!(before.get("paused").unwrap().as_bool(), Some(true));

        let accepted = body(dispatch(
            &request(
                "POST",
                &format!("/v1/worlds/{id}/events"),
                r#"{"events":[{"type":"spawn_sphere","material":"steel","pos":[0,2,0],"radius":0.1}]}"#,
            ),
            &state,
        ));
        assert_eq!(accepted.get("accepted").unwrap().as_u64(), Some(1));

        let full_before = body(dispatch(
            &request("GET", &format!("/v1/worlds/{id}/state/full"), ""),
            &state,
        ));
        assert_eq!(
            full_before
                .get("bodies")
                .unwrap()
                .get("n")
                .unwrap()
                .as_u64(),
            Some(0)
        );

        let flushed = body(dispatch(
            &request(
                "POST",
                &format!("/v1/worlds/{id}/control"),
                r#"{"action":"flush"}"#,
            ),
            &state,
        ));
        assert_eq!(flushed.get("paused").unwrap().as_bool(), Some(true));

        let full = body(dispatch(
            &request("GET", &format!("/v1/worlds/{id}/state/full"), ""),
            &state,
        ));
        assert_eq!(
            full.get("bodies").unwrap().get("n").unwrap().as_u64(),
            Some(1)
        );
        assert!(!full
            .get("bodies")
            .unwrap()
            .get("py")
            .unwrap()
            .as_str()
            .unwrap()
            .is_empty());

        let stepped = body(dispatch(
            &request("POST", &format!("/v1/worlds/{id}/step"), r#"{"ticks":3}"#),
            &state,
        ));
        assert_eq!(stepped.get("stepped").unwrap().as_u64(), Some(3));
        assert_eq!(
            stepped.get("state").unwrap().get("tick").unwrap().as_u64(),
            Some(3)
        );
        assert_eq!(
            stepped
                .get("observation")
                .unwrap()
                .get("bodies")
                .unwrap()
                .get("n")
                .unwrap()
                .as_u64(),
            Some(1)
        );
    }

    #[test]
    fn nbody_and_batch_contracts_are_real() {
        let state = AppState::new();
        let nbody = body(dispatch(
            &request(
                "POST",
                "/v1/worlds",
                r#"{"config":{"auto_start":false,"gravity":[0,0,0],"container":false,"systems":{"fluid":{"enabled":false},"heat":{"enabled":false},"nbody":{"enabled":true}}}}"#,
            ),
            &state,
        ));
        let id = nbody.get("id").unwrap().as_str().unwrap().to_string();
        let _ = body(dispatch(
            &request(
                "POST",
                &format!("/v1/worlds/{id}/events"),
                r#"{"events":[{"type":"spawn_nbody","mass":10000000000,"pos":[-1,0,0],"vel":[0,0.4,0]},{"type":"spawn_nbody","mass":10000000000,"pos":[1,0,0],"vel":[0,-0.4,0]}]}"#,
            ),
            &state,
        ));
        let _ = body(dispatch(
            &request(
                "POST",
                &format!("/v1/worlds/{id}/control"),
                r#"{"action":"flush"}"#,
            ),
            &state,
        ));
        let stepped = body(dispatch(
            &request("POST", &format!("/v1/worlds/{id}/step"), r#"{"ticks":2}"#),
            &state,
        ));
        assert_eq!(
            stepped
                .get("observation")
                .unwrap()
                .get("nbody")
                .unwrap()
                .get("n")
                .unwrap()
                .as_u64(),
            Some(2)
        );

        let batch = body(dispatch(
            &request(
                "POST",
                "/v1/batch/create",
                r#"{"count":3,"name_prefix":"contract","template_config":{"quality":"low","auto_start":true,"systems":{"fluid":{"enabled":false},"heat":{"enabled":false}}}}"#,
            ),
            &state,
        ));
        let ids = batch.get("ids").unwrap().as_arr().unwrap();
        assert_eq!(ids.len(), 3);
        let batch_step_body = format!(
            r#"{{"ids":{},"ticks_per_world":2,"include_observation":true}}"#,
            Json::Arr(ids.to_vec()).dump()
        );
        let stepped_batch = body(dispatch(
            &request("POST", "/v1/batch/step", &batch_step_body),
            &state,
        ));
        let results = stepped_batch.get("results").unwrap().as_arr().unwrap();
        assert_eq!(results.len(), 3);
        assert!(results
            .iter()
            .all(|r| r.get("tick").unwrap().as_u64() == Some(2) && r.get("observation").is_some()));

        // Different actions for two environments are processed by one atomic
        // transition request, the API primitive used by vectorized RL.
        let transition_body = format!(
            r#"{{"transitions":[{{"id":"{}","events":[{{"type":"apply_force","target":{{"body":0}},"force":[1,0,0]}}]}},{{"id":"{}","events":[]}}],"ticks_per_world":1,"include_observation":true}}"#,
            ids[0].as_str().unwrap(),
            ids[1].as_str().unwrap()
        );
        let transition = body(dispatch(
            &request("POST", "/v1/batch/transition", &transition_body),
            &state,
        ));
        assert_eq!(transition.get("count").unwrap().as_u64(), Some(2));
        assert!(transition
            .get("results")
            .unwrap()
            .as_arr()
            .unwrap()
            .iter()
            .all(|r| r.get("observation").is_some()));
    }
}
