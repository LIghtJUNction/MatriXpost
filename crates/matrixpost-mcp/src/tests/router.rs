use super::*;

#[tokio::test]
async fn macro_generated_router_returns_a_tool_error_for_unknown_input_fields() {
    let (client, server_handle) = connect(service()).await;
    let result = client
        .call_tool(
            CallToolRequestParams::new("publish_video").with_arguments(
                serde_json::json!({
                    "platform": "dy",
                    "file": "/tmp/video.mp4",
                    "title": "Title",
                    "phone": "13800138000",
                    "cookie": "must-not-be-accepted"
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(true));
    let message = result
        .content
        .first()
        .and_then(|content| content.as_text())
        .map(|content| content.text.as_str())
        .unwrap();
    assert!(message.contains("unknown field `cookie`"));
    disconnect(client, server_handle).await;
}

#[tokio::test]
async fn lifecycle_router_rejects_unknown_fields_and_creates_then_lists_objects() {
    let (client, server_handle) = connect(service()).await;
    let rejected = client
        .call_tool(
            CallToolRequestParams::new("list_business_objects").with_arguments(
                serde_json::json!({"unexpected": true})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(rejected.is_error, Some(true));

    let created = client
        .call_tool(
            CallToolRequestParams::new("create_business_object").with_arguments(
                serde_json::json!({
                    "id": "campaign-1",
                    "kind": "campaign",
                    "displayName": "Launch campaign",
                    "externalId": "external-1",
                    "attributes": {"region": "east"}
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(created.is_error, Some(false));
    assert_eq!(
        created.structured_content.as_ref().unwrap()["lifecycle_status"],
        "draft"
    );
    assert_eq!(
        created.structured_content.as_ref().unwrap()["approval_status"],
        "pending"
    );

    let listed = client
        .call_tool(CallToolRequestParams::new("list_business_objects"))
        .await
        .unwrap();
    assert_eq!(listed.is_error, Some(false));
    assert_eq!(
        listed.structured_content,
        Some(serde_json::json!([{
            "id": "campaign-1",
            "kind": "campaign",
            "external_id": "external-1",
            "display_name": "Launch campaign",
            "lifecycle_status": "draft",
            "approval_status": "pending",
            "revision": 0,
            "attributes": {"region": "east"},
            "created_at": created.structured_content.as_ref().unwrap()["created_at"],
            "updated_at": created.structured_content.as_ref().unwrap()["updated_at"]
        }]))
    );
    disconnect(client, server_handle).await;
}

#[tokio::test]
async fn lifecycle_router_appends_ledger_entries_and_hides_missing_object_details() {
    let (client, server_handle) = connect(service()).await;
    let missing = client
        .call_tool(
            CallToolRequestParams::new("append_ledger_entry").with_arguments(
                serde_json::json!({
                    "id": "entry-missing",
                    "businessObjectId": "missing",
                    "direction": "expense",
                    "category": "service",
                    "amountMinor": 1250,
                    "currency": "CNY"
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(missing.is_error, Some(true));
    assert_eq!(
        missing.structured_content,
        Some(serde_json::json!({
            "outcome": "rejected",
            "code": "not_found",
            "message": "the requested lifecycle record does not exist"
        }))
    );
    disconnect(client, server_handle).await;

    let service = service();
    service
        .create_business_object_result(CreateBusinessObjectInput {
            id: "asset-1".into(),
            kind: "asset".into(),
            display_name: "Asset".into(),
            external_id: None,
            lifecycle_status: None,
            approval_status: None,
            attributes: None,
        })
        .unwrap();
    let (client, server_handle) = connect(service).await;
    let appended = client
        .call_tool(
            CallToolRequestParams::new("append_ledger_entry").with_arguments(
                serde_json::json!({
                    "id": "entry-1",
                    "businessObjectId": "asset-1",
                    "direction": "revenue",
                    "category": "sale",
                    "amountMinor": 4500,
                    "currency": "CNY",
                    "approvalStatus": "approved"
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(appended.is_error, Some(false));
    let listed = client
        .call_tool(
            CallToolRequestParams::new("list_ledger_entries").with_arguments(
                serde_json::json!({"businessObjectId": "asset-1"})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(listed.is_error, Some(false));
    assert_eq!(
        listed
            .structured_content
            .as_ref()
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        listed.structured_content.as_ref().unwrap()[0]["amount_minor"],
        4500
    );
    disconnect(client, server_handle).await;
}

#[tokio::test]
async fn lifecycle_child_lists_reject_missing_objects_instead_of_returning_empty_arrays() {
    let (client, server_handle) = connect(service()).await;

    for tool_name in ["list_ledger_entries", "list_content_attributions"] {
        let response = client
            .call_tool(
                CallToolRequestParams::new(tool_name).with_arguments(
                    serde_json::json!({"businessObjectId": "missing-object"})
                        .as_object()
                        .unwrap()
                        .clone(),
                ),
            )
            .await
            .unwrap();
        assert_eq!(response.is_error, Some(true), "{tool_name}");
        assert_eq!(
            response.structured_content,
            Some(serde_json::json!({
                "outcome": "rejected",
                "code": "not_found",
                "message": "the requested lifecycle record does not exist"
            })),
            "{tool_name}"
        );
    }

    disconnect(client, server_handle).await;
}

#[tokio::test]
async fn lifecycle_router_rejects_attribution_to_missing_history() {
    let service = service();
    service
        .create_business_object_result(CreateBusinessObjectInput {
            id: "project-1".into(),
            kind: "project".into(),
            display_name: "Project".into(),
            external_id: None,
            lifecycle_status: None,
            approval_status: None,
            attributes: None,
        })
        .unwrap();
    let (client, server_handle) = connect(service).await;
    let result = client
        .call_tool(
            CallToolRequestParams::new("add_content_attribution").with_arguments(
                serde_json::json!({
                    "businessObjectId": "project-1",
                    "historyId": "missing-history"
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(true));
    assert_eq!(
        result.structured_content,
        Some(serde_json::json!({
            "outcome": "rejected",
            "code": "not_found",
            "message": "the requested lifecycle record does not exist"
        }))
    );
    disconnect(client, server_handle).await;
}

#[tokio::test]
async fn lifecycle_router_creates_and_lists_safe_generic_business_relations() {
    let service = service();
    for (id, kind, display_name) in [
        ("asset-1", "asset", "Asset"),
        ("customer-1", "customer", "Customer"),
    ] {
        service
            .create_business_object_result(CreateBusinessObjectInput {
                id: id.into(),
                kind: kind.into(),
                display_name: display_name.into(),
                external_id: None,
                lifecycle_status: None,
                approval_status: None,
                attributes: None,
            })
            .unwrap();
    }
    let (client, server_handle) = connect(service).await;
    let created = client
        .call_tool(
            CallToolRequestParams::new("add_business_relation").with_arguments(
                serde_json::json!({
                    "id": "interest-1",
                    "sourceBusinessObjectId": "asset-1",
                    "targetBusinessObjectId": "customer-1",
                    "relationType": "customer_interest",
                    "attributes": {"priority": "high"}
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(created.is_error, Some(false));
    assert_eq!(
        created.structured_content.as_ref().unwrap()["relation_type"],
        "customer_interest"
    );
    assert_eq!(
        created.structured_content.as_ref().unwrap()["attributes"]["priority"],
        "high"
    );

    let listed = client
        .call_tool(
            CallToolRequestParams::new("list_business_relations").with_arguments(
                serde_json::json!({"businessObjectId": "customer-1"})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(listed.is_error, Some(false));
    assert_eq!(
        listed
            .structured_content
            .as_ref()
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        listed.structured_content.as_ref().unwrap()[0]["source_business_object_id"],
        "asset-1"
    );
    disconnect(client, server_handle).await;
}

#[tokio::test]
async fn lifecycle_relation_tools_reject_unknown_input_and_missing_objects() {
    let (client, server_handle) = connect(service()).await;
    let unknown_field = client
        .call_tool(
            CallToolRequestParams::new("list_business_relations").with_arguments(
                serde_json::json!({
                    "businessObjectId": "missing-object",
                    "unexpected": true
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(unknown_field.is_error, Some(true));

    let missing = client
        .call_tool(
            CallToolRequestParams::new("add_business_relation").with_arguments(
                serde_json::json!({
                    "id": "interest-missing",
                    "sourceBusinessObjectId": "missing-source",
                    "targetBusinessObjectId": "missing-target",
                    "relationType": "customer_interest"
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(missing.is_error, Some(true));
    assert_eq!(
        missing.structured_content,
        Some(serde_json::json!({
            "outcome": "rejected",
            "code": "not_found",
            "message": "the requested lifecycle record does not exist"
        }))
    );
    disconnect(client, server_handle).await;
}

#[tokio::test]
async fn lifecycle_router_transitions_legally_and_rejects_stale_revisions() {
    let service = service();
    service
        .create_business_object_result(CreateBusinessObjectInput {
            id: "project-2".into(),
            kind: "project".into(),
            display_name: "Project".into(),
            external_id: None,
            lifecycle_status: None,
            approval_status: None,
            attributes: None,
        })
        .unwrap();
    let (client, server_handle) = connect(service).await;
    let transitioned = client
        .call_tool(
            CallToolRequestParams::new("transition_business_object").with_arguments(
                serde_json::json!({
                    "id": "project-2",
                    "expectedRevision": 0,
                    "lifecycleStatus": "active",
                    "approvalStatus": "pending"
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(transitioned.is_error, Some(false));
    assert_eq!(
        transitioned.structured_content.as_ref().unwrap()["revision"],
        1
    );

    let stale = client
        .call_tool(
            CallToolRequestParams::new("transition_business_object").with_arguments(
                serde_json::json!({
                    "id": "project-2",
                    "expectedRevision": 0,
                    "lifecycleStatus": "completed",
                    "approvalStatus": "approved"
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(stale.is_error, Some(true));
    assert_eq!(
        stale.structured_content,
        Some(serde_json::json!({
            "outcome": "rejected",
            "code": "invalid_input",
            "message": "the lifecycle input is invalid or conflicts with existing state"
        }))
    );
    disconnect(client, server_handle).await;
}

#[tokio::test]
async fn macro_generated_router_rejects_fqsp_history_filter() {
    let (client, server_handle) = connect(service()).await;
    let result = client
        .call_tool(
            CallToolRequestParams::new("list_history").with_arguments(
                serde_json::json!({"platform":"fqsp"})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(true));
    disconnect(client, server_handle).await;
}

#[tokio::test]
async fn macro_generated_router_lists_no_accounts_from_fresh_state() {
    let (client, server_handle) = connect(service()).await;
    let result = client
        .call_tool(CallToolRequestParams::new("list_accounts"))
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(false));
    assert_eq!(result.structured_content, Some(serde_json::json!([])));
    disconnect(client, server_handle).await;
}

#[tokio::test]
async fn macro_generated_router_returns_persisted_juejin_account_as_structured_array() {
    let service = service();
    service
        .repository
        .save_article_account(&ArticleAccount {
            id: "j".into(),
            platform: ArticlePlatform::Juejin,
            display_name: "Primary".into(),
            status: ArticleAccountStatus::LoggedIn,
            phone: "13800138000".into(),
            partition: "persist:j".into(),
        })
        .unwrap();
    let (client, server_handle) = connect(service).await;
    let result = client
        .call_tool(
            CallToolRequestParams::new("list_accounts").with_arguments(
                serde_json::json!({"platform":"juejin"})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(
        result.structured_content,
        Some(
            serde_json::json!([{"phone":"13800138000","platform":"juejin","partition":"persist:j"}])
        )
    );
    disconnect(client, server_handle).await;
}
