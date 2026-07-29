use super::support::*;

#[tokio::test]
async fn lifecycle_object_routes_create_list_and_get() {
    let router = lifecycle_router();
    let (status, body) = json_response(
        router.clone(),
        lifecycle_request(
            "POST",
            "/lifecycle/objects",
            lifecycle_object_payload("object-1"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["outcome"], "created");
    assert_eq!(body["data"]["id"], "object-1");

    let (status, body) = json_response(
        router.clone(),
        Request::get("/lifecycle/objects")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"].as_array().unwrap().len(), 1);

    let (status, body) = json_response(
        router,
        Request::get("/lifecycle/objects/object-1")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["display_name"], "Example object");
}

#[tokio::test]
async fn lifecycle_transition_route_updates_an_object_and_rejects_stale_revisions() {
    let router = lifecycle_router();
    let (status, _) = json_response(
        router.clone(),
        lifecycle_request(
            "POST",
            "/lifecycle/objects",
            lifecycle_object_payload("object-1"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let transition = serde_json::json!({
        "expected_revision": 0,
        "lifecycle_status": "completed",
        "approval_status": "approved",
        "updated_at": "2026-07-29T01:00:00Z"
    });
    let (status, body) = json_response(
        router.clone(),
        lifecycle_request(
            "POST",
            "/lifecycle/objects/object-1/transition",
            transition.clone(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["outcome"], "ok");
    assert_eq!(body["data"]["lifecycle_status"], "completed");
    assert_eq!(body["data"]["revision"], 1);

    let (status, body) = json_response(
        router,
        lifecycle_request("POST", "/lifecycle/objects/object-1/transition", transition),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["outcome"], "rejected");
    assert_eq!(body["message"], "lifecycle request could not be completed");
}

#[tokio::test]
async fn lifecycle_transition_route_rejects_unknown_fields_and_missing_objects() {
    let router = lifecycle_router();
    let mut unknown_field = serde_json::json!({
        "expected_revision": 0,
        "lifecycle_status": "completed",
        "approval_status": "approved",
        "updated_at": "2026-07-29T01:00:00Z"
    });
    unknown_field["unexpected"] = serde_json::json!(true);
    let (status, body) = json_response(
        router.clone(),
        lifecycle_request(
            "POST",
            "/lifecycle/objects/missing/transition",
            unknown_field,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["outcome"], "rejected");
    assert!(body["message"].as_str().unwrap().contains("unknown"));

    let (status, body) = json_response(
        router,
        lifecycle_request(
            "POST",
            "/lifecycle/objects/missing/transition",
            serde_json::json!({
                "expected_revision": 0,
                "lifecycle_status": "completed",
                "approval_status": "approved",
                "updated_at": "2026-07-29T01:00:00Z"
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["outcome"], "not_found");
    assert_eq!(body["message"], "business object was not found");
}

#[tokio::test]
async fn lifecycle_rejects_unknown_fields_and_unknown_objects_are_404() {
    let router = lifecycle_router();
    let mut payload = lifecycle_object_payload("object-1");
    payload["unexpected"] = serde_json::json!(true);
    let (status, body) = json_response(
        router.clone(),
        lifecycle_request("POST", "/lifecycle/objects", payload),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["outcome"], "rejected");
    assert!(body["message"].as_str().unwrap().contains("unknown"));

    let (status, body) = json_response(
        router.clone(),
        Request::get("/lifecycle/objects/missing")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["outcome"], "not_found");

    let (status, _) = json_response(
        router,
        Request::get("/lifecycle/objects/missing/ledger")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn lifecycle_ledger_route_rejects_path_mismatch_and_appends_entries() {
    let router = lifecycle_router();
    let (status, _) = json_response(
        router.clone(),
        lifecycle_request(
            "POST",
            "/lifecycle/objects",
            lifecycle_object_payload("object-1"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let mismatched = serde_json::json!({
        "id": "ledger-1", "business_object_id": "other", "direction": "expense",
        "category": "service", "amount_minor": 35000, "currency": "CNY",
        "occurred_at": "2026-07-29T00:00:00Z", "approval_status": "approved",
        "counterparty": null, "reference": null, "description": null,
        "created_at": "2026-07-29T00:00:00Z"
    });
    let (status, body) = json_response(
        router.clone(),
        lifecycle_request("POST", "/lifecycle/objects/object-1/ledger", mismatched),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["message"].as_str().unwrap().contains("must match"));

    let entry = serde_json::json!({
        "id": "ledger-1", "business_object_id": "object-1", "direction": "expense",
        "category": "service", "amount_minor": 35000, "currency": "CNY",
        "occurred_at": "2026-07-29T00:00:00Z", "approval_status": "approved",
        "counterparty": null, "reference": null, "description": null,
        "created_at": "2026-07-29T00:00:00Z"
    });
    let (status, body) = json_response(
        router.clone(),
        lifecycle_request("POST", "/lifecycle/objects/object-1/ledger", entry),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["data"]["id"], "ledger-1");

    let (status, body) = json_response(
        router,
        Request::get("/lifecycle/objects/object-1/ledger")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn lifecycle_attribution_route_rejects_path_mismatch_and_missing_history() {
    let router = lifecycle_router();
    let (status, _) = json_response(
        router.clone(),
        lifecycle_request(
            "POST",
            "/lifecycle/objects",
            lifecycle_object_payload("object-1"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = json_response(
        router.clone(),
        lifecycle_request(
            "POST",
            "/lifecycle/objects/object-1/attributions",
            serde_json::json!({
                "business_object_id": "other", "history_id": "history-1",
                "created_at": "2026-07-29T00:00:00Z"
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["message"].as_str().unwrap().contains("must match"));

    let (status, body) = json_response(
        router,
        lifecycle_request(
            "POST",
            "/lifecycle/objects/object-1/attributions",
            serde_json::json!({
                "business_object_id": "object-1", "history_id": "missing-history",
                "created_at": "2026-07-29T00:00:00Z"
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["outcome"], "rejected");
}

#[tokio::test]
async fn lifecycle_relation_routes_create_and_list_directed_relations() {
    let router = lifecycle_router();
    for (id, external_id) in [
        ("asset-1", "asset-external"),
        ("customer-1", "customer-external"),
    ] {
        let mut object = lifecycle_object_payload(id);
        object["external_id"] = serde_json::json!(external_id);
        let (status, _) = json_response(
            router.clone(),
            lifecycle_request("POST", "/lifecycle/objects", object),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }

    let relation = serde_json::json!({
        "id": "relation-1",
        "sourceBusinessObjectId": "asset-1",
        "targetBusinessObjectId": "customer-1",
        "relationType": "customer_interest",
        "attributes": { "priority": "high" }
    });
    let (status, body) = json_response(
        router.clone(),
        lifecycle_request("POST", "/lifecycle/objects/asset-1/relations", relation),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["data"]["source_business_object_id"], "asset-1");
    assert_eq!(body["data"]["target_business_object_id"], "customer-1");

    let (status, body) = json_response(
        router.clone(),
        Request::get("/lifecycle/objects/asset-1/relations")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"].as_array().unwrap().len(), 1);

    let (status, body) = json_response(
        router,
        Request::get("/lifecycle/objects/customer-1/relations")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn lifecycle_relation_routes_reject_unknown_fields_mismatches_and_missing_objects() {
    let router = lifecycle_router();
    let (status, _) = json_response(
        router.clone(),
        lifecycle_request(
            "POST",
            "/lifecycle/objects",
            lifecycle_object_payload("object-1"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let unknown = serde_json::json!({
        "id": "relation-1",
        "sourceBusinessObjectId": "object-1",
        "targetBusinessObjectId": "missing-target",
        "relationType": "owner",
        "unexpected": true
    });
    let (status, body) = json_response(
        router.clone(),
        lifecycle_request("POST", "/lifecycle/objects/object-1/relations", unknown),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["message"].as_str().unwrap().contains("unknown"));

    let mismatched = serde_json::json!({
        "id": "relation-1",
        "sourceBusinessObjectId": "other-object",
        "targetBusinessObjectId": "missing-target",
        "relationType": "owner"
    });
    let (status, body) = json_response(
        router.clone(),
        lifecycle_request("POST", "/lifecycle/objects/object-1/relations", mismatched),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["message"].as_str().unwrap().contains("must match"));

    let (status, body) = json_response(
        router.clone(),
        Request::get("/lifecycle/objects/missing/relations")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["outcome"], "not_found");

    let missing_target = serde_json::json!({
        "id": "relation-1",
        "sourceBusinessObjectId": "object-1",
        "targetBusinessObjectId": "missing-target",
        "relationType": "owner"
    });
    let (status, body) = json_response(
        router,
        lifecycle_request(
            "POST",
            "/lifecycle/objects/object-1/relations",
            missing_target,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["outcome"], "not_found");
}
