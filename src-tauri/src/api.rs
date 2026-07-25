// ITSM API 封装（纯 HTTP 层，不依赖 Tauri State）
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const API_BASE: &str = "https://api-itsm.chinasie.com";

/// token 失效/权限不通过时，后端返给前端的统一错误标识（前端据此 showLogin）
pub const AUTH_EXPIRED_ERR: &str = "登录已失效";

/// ITSM 网关对失效 token 返回 HTTP 200 + 业务码 body，靠此字段识别。
/// 已验证响应：{"code":"-1","msgCode":"1011_common_119","status":"PERMISSION_NOT_PASS"}
fn is_permission_not_pass(v: &Value) -> bool {
    v.get("status")
        .and_then(|s| s.as_str())
        .map_or(false, |s| s == "PERMISSION_NOT_PASS")
}

pub async fn do_get(
    client: &reqwest::Client,
    token: &str,
    path: &str,
) -> Result<Value, String> {
    let url = format!("{}{}", API_BASE, path);
    let v: Value = client
        .get(&url)
        .header("authorization", token)
        .header("language", "zh#cn")
        .send()
        .await
        .map_err(|e| format!("请求失败: {}", e))?
        .json()
        .await
        .map_err(|e| format!("解析失败: {}", e))?;
    if is_permission_not_pass(&v) {
        return Err(AUTH_EXPIRED_ERR.into());
    }
    Ok(v)
}

pub async fn do_post(
    client: &reqwest::Client,
    token: &str,
    path: &str,
    body: Value,
) -> Result<Value, String> {
    let url = format!("{}{}", API_BASE, path);
    let v: Value = client
        .post(&url)
        .header("authorization", token)
        .header("language", "zh#cn")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("请求失败: {}", e))?
        .json()
        .await
        .map_err(|e| format!("解析失败: {}", e))?;
    if is_permission_not_pass(&v) {
        return Err(AUTH_EXPIRED_ERR.into());
    }
    Ok(v)
}

pub async fn list_views(client: &reqwest::Client, token: &str) -> Result<Value, String> {
    do_get(
        client,
        token,
        "/api/itsm/incidentViewBase/findMyViewList?containCount=true&type=1",
    )
    .await
}

/// 列表搜索参数（全可选；未填字段不加入请求 params）
/// 字段名按 ITSM find-pagination 后端 key（camelCase），前端 invoke 时用 camelCase 传。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchParams {
    pub code_and_subject: Option<String>,
    pub status: Option<String>,
    pub creation_date_begin: Option<String>,
    pub creation_date_end: Option<String>,
    pub contact_customer_group_name: Option<String>,
}

impl SearchParams {
    /// 全字段均为空 → 视作无搜索条件（走缓存默认逻辑）
    pub fn is_empty(&self) -> bool {
        let blank = |s: &Option<String>| s.as_deref().map_or(true, |v| v.is_empty());
        blank(&self.code_and_subject)
            && blank(&self.status)
            && blank(&self.creation_date_begin)
            && blank(&self.creation_date_end)
            && blank(&self.contact_customer_group_name)
    }

    /// 合成 params 子对象（仅含非空字段；不含 seachType，由调用方注入）
    pub fn to_params(&self) -> Value {
        let mut m = serde_json::Map::new();
        if let Some(s) = self.code_and_subject.as_deref().filter(|s| !s.is_empty()) {
            m.insert("codeAndSubject".into(), Value::String(s.to_string()));
        }
        if let Some(s) = self.status.as_deref().filter(|s| !s.is_empty()) {
            m.insert("status".into(), Value::String(s.to_string()));
        }
        let b = self.creation_date_begin.as_deref().filter(|s| !s.is_empty());
        let e = self.creation_date_end.as_deref().filter(|s| !s.is_empty());
        // 日期需成对（UI 层强制成对输入），单边不发送避免后端歧义
        if let (Some(b), Some(e)) = (b, e) {
            m.insert(
                "creationDateSearch".into(),
                Value::Array(vec![Value::String(b.to_string()), Value::String(e.to_string())]),
            );
        }
        if let Some(s) = self.contact_customer_group_name.as_deref().filter(|s| !s.is_empty()) {
            m.insert("contactCustomerGroupName".into(), Value::String(s.to_string()));
        }
        Value::Object(m)
    }
}

/// 把 search 字段 merge 进已含 seachType 的 params map
fn merge_search(params: &mut serde_json::Map<String, Value>, search: Option<&SearchParams>) {
    if let Some(s) = search {
        if let Value::Object(m) = s.to_params() {
            for (k, v) in m {
                params.insert(k, v);
            }
        }
    }
}

pub async fn list_tickets(
    client: &reqwest::Client,
    token: &str,
    seach_type: i64,
    search: Option<&SearchParams>,
) -> Result<Value, String> {
    let mut params = serde_json::Map::new();
    params.insert("seachType".into(), json!(seach_type));
    merge_search(&mut params, search);
    let body = json!({
        "pageIndex": 1, "pageRows": 200,
        "params": params,
        "orderByBean": { "attributeName": "", "sortType": "" }
    });
    do_post(client, token, "/api/itsm/incidentService/find-pagination", body).await
}

pub async fn get_detail(
    client: &reqwest::Client,
    token: &str,
    id: &str,
) -> Result<Value, String> {
    do_get(
        client,
        token,
        &format!("/api/itsm/incidentService/get-with-fields?id={}", id),
    )
    .await
}

pub async fn list_replies(
    client: &reqwest::Client,
    token: &str,
    incident_id: &str,
) -> Result<Value, String> {
    do_get(
        client,
        token,
        &format!("/api/itsm/incidentService/find-replyList?incidentId={}", incident_id),
    )
    .await
}

pub async fn claim(client: &reqwest::Client, token: &str, id: &str) -> Result<Value, String> {
    let url = format!("{}/api/itsm/incidentService/snatch-order", API_BASE);
    let form = reqwest::multipart::Form::new().text("id", id.to_string());
    client
        .post(&url)
        .header("authorization", token)
        .header("language", "zh#cn")
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("接单失败: {}", e))?
        .json::<Value>()
        .await
        .map_err(|e| format!("解析失败: {}", e))
}

/// 上传结果（序列化回前端）
#[derive(Debug, Clone, Serialize)]
pub struct UploadResult {
    pub file_id: String,
    pub file_path: String,
    pub file_name: String,
}

/// multipart POST 基础函数（header 同 do_post，body 为 multipart form）
pub async fn do_post_multipart(
    client: &reqwest::Client,
    token: &str,
    path: &str,
    form: reqwest::multipart::Form,
) -> Result<Value, String> {
    let url = format!("{}{}", API_BASE, path);
    let v = client
        .post(&url)
        .header("authorization", token)
        .header("language", "zh#cn")
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("上传失败: {}", e))?
        .json::<Value>()
        .await
        .map_err(|e| format!("解析失败: {}", e))?;
    if is_permission_not_pass(&v) {
        return Err(AUTH_EXPIRED_ERR.into());
    }
    Ok(v)
}

/// 纯解析：上传响应 → UploadResult（成功码 800）
pub fn parse_upload_response(v: &Value) -> Result<UploadResult, String> {
    let code = v.get("code").and_then(|c| c.as_i64()).unwrap_or(0);
    if code != 800 {
        return Err(format!("上传失败: {}", v.get("msg").and_then(|m| m.as_str()).unwrap_or("")));
    }
    let data = v.get("data").ok_or_else(|| "上传响应缺 data".to_string())?;
    let file_id = data.get("fileId").and_then(|x| x.as_str()).ok_or_else(|| "上传响应缺 fileId".to_string())?.to_string();
    let file_path = data.get("filePath").and_then(|x| x.as_str()).ok_or_else(|| "上传响应缺 filePath".to_string())?.to_string();
    let file_name = data.get("sourceFileName").and_then(|x| x.as_str()).unwrap_or("").to_string();
    Ok(UploadResult { file_id, file_path, file_name })
}

/// 上传单个附件到 ITSM（预上传，bizId 空）
pub async fn upload_attachment(
    client: &reqwest::Client,
    token: &str,
    bytes: Vec<u8>,
    file_name: &str,
    mime: &str,
) -> Result<UploadResult, String> {
    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name(file_name.to_string())
        .mime_str(mime)
        .map_err(|e| format!("mime 错误: {}", e))?;
    let form = reqwest::multipart::Form::new().part("file", part);
    let v = do_post_multipart(client, token, "/api/file/iot-base-attachment/single/file-upload?bizId=", form).await?;
    parse_upload_response(&v)
}

/// 纯构造：回复请求 body（便于单测）
pub fn reply_body(
    order_id: &str,
    detail_html: &str,
    file_ids: &[String],
    is_private: bool,
    order_type: &str,
) -> Value {
    json!({
        "params": {
            "orderId": order_id, "profile": "",
            "fileIds": file_ids,
            "orderType": order_type,
            "detail": detail_html,
            "isPrivate": if is_private { 1 } else { 0 },
            "operationSource": "ITSM_DEVOPS", "replyType": 1
        }
    })
}

pub async fn reply(
    client: &reqwest::Client,
    token: &str,
    order_id: &str,
    detail_html: &str,
    file_ids: &[String],
    is_private: bool,
    order_type: &str,
) -> Result<Value, String> {
    let body = reply_body(order_id, detail_html, file_ids, is_private, order_type);
    do_post(client, token, "/api/itsm/incidentService/order-reply", body).await
}

pub async fn change_status(
    client: &reqwest::Client,
    token: &str,
    id: &str,
    status: &str,
    content: &str,
    is_resolve: bool,
) -> Result<Value, String> {
    let mut data = do_get(
        client,
        token,
        &format!("/api/itsm/incidentService/get-with-fields?id={}", id),
    )
    .await?
    .get("data")
    .cloned()
    .ok_or_else(|| "未取到工单数据".to_string())?;
    if let Some(obj) = data.as_object_mut() {
        obj.insert("status".into(), Value::String(status.to_string()));
        obj.insert("solution".into(), Value::String(content.to_string()));
        if is_resolve {
            obj.insert("statusName".into(), Value::String("已解决".into()));
            obj.insert("endCode".into(), Value::String("1".into()));
            obj.insert("accountability".into(), Value::String("3".into()));
            obj.insert("statusIndex".into(), json!(3));
        }
    } else {
        return Err("工单数据格式异常".into());
    }
    do_post(client, token, "/api/itsm/incidentService/update", json!({ "params": data })).await
}

// ============ 补单 / 转派 / 取消 / 关闭 ============

/// 服务目录树（3 级 cascader 数据源）
pub async fn list_service_tree(
    client: &reqwest::Client,
    token: &str,
) -> Result<Value, String> {
    do_get(
        client,
        token,
        "/api/bussconfig/service-config/findServiceTypeTree2?filterPermission=false&status=0&type=1",
    )
    .await
}

/// 补单模板（叶子服务目录 id → 模板字段 + createTemplateId）
pub async fn get_replenish_template(
    client: &reqwest::Client,
    token: &str,
    leaf_id: &str,
) -> Result<Value, String> {
    do_get(
        client,
        token,
        &format!("/api/bussconfig/service-config/template/replenish?id={}", leaf_id),
    )
    .await
}

/// 字典：dicType（PRIORITY / Request_Source / REQUEST_TYPE / Influence_Degree / Urgent_Degree / itsm_incident_allot_type）
pub async fn get_dict(
    client: &reqwest::Client,
    token: &str,
    tenant_id: &str,
    dic_type: &str,
) -> Result<Value, String> {
    do_get(
        client,
        token,
        &format!(
            "/dictionary/dic-items/get-dictype-notoken?dicType={}&tenantId={}",
            dic_type, tenant_id
        ),
    )
    .await
}

/// 全支持组列表
pub async fn list_support_groups(
    client: &reqwest::Client,
    token: &str,
) -> Result<Value, String> {
    do_post(
        client,
        token,
        "/api/bussconfig/support-group/find-list",
        json!({ "params": { "groupState": 0 } }),
    )
    .await
}

/// 全支持组成员（前端按 sgId 过滤出某组成员）
pub async fn list_support_members(
    client: &reqwest::Client,
    token: &str,
) -> Result<Value, String> {
    do_post(
        client,
        token,
        "/api/bussconfig/support-group-member/find-alllist",
        json!({ "params": {} }),
    )
    .await
}

/// 客户组搜索（关键字为空时返回首页列表）
pub async fn search_customer_groups(
    client: &reqwest::Client,
    token: &str,
    keyword: &str,
) -> Result<Value, String> {
    do_post(
        client,
        token,
        "/api/bussconfig/customer-group/find-pagination",
        json!({
            "pageIndex": 1,
            "pageRows": 20,
            "params": { "groupState": "0", "customerGroupName": keyword }
        }),
    )
    .await
}

/// 人员搜索（关键字为空时返回首页列表）
pub async fn search_base_persons(
    client: &reqwest::Client,
    token: &str,
    keyword: &str,
) -> Result<Value, String> {
    do_post(
        client,
        token,
        "/api/base/base-person/find-pagination-all",
        json!({
            "pageIndex": 1,
            "pageRows": 20,
            "params": { "useState": "0", "psnName": keyword }
        }),
    )
    .await
}

/// 补单提交：params 为前端组装的完整表单
pub async fn save_replenish(
    client: &reqwest::Client,
    token: &str,
    params: Value,
) -> Result<Value, String> {
    do_post(
        client,
        token,
        "/api/itsm/incidentService/save",
        json!({ "params": params }),
    )
    .await
}

/// 转派提交：params 为前端组装的 {incidentId, assign, supportBy, allotType, ...}
pub async fn reassign(
    client: &reqwest::Client,
    token: &str,
    params: Value,
) -> Result<Value, String> {
    do_post(
        client,
        token,
        "/api/itsm/incident-bpm/order-reply",
        json!({ "params": params }),
    )
    .await
}

/// 暂挂/取消挂起：multipart form { id, optionType ("suspend"|"unhang"), detail }
/// 后端据 detail 自动写系统回复「XXX挂起工单，挂起原因：{detail}」
pub async fn suspend_or_unhang(
    client: &reqwest::Client,
    token: &str,
    id: &str,
    option_type: &str,
    detail: &str,
) -> Result<Value, String> {
    let form = reqwest::multipart::Form::new()
        .text("id", id.to_string())
        .text("optionType", option_type.to_string())
        .text("detail", detail.to_string());
    do_post_multipart(client, token, "/api/itsm/incidentService/suspend-or-unhang", form).await
}

/// 取消工单：无 params 包裹，body 直接是 {incidentId, operationReason}
pub async fn cancel_incident(
    client: &reqwest::Client,
    token: &str,
    incident_id: &str,
    reason: &str,
) -> Result<Value, String> {
    do_post(
        client,
        token,
        "/api/itsm/incidentAction/delete-incident",
        json!({ "incidentId": incident_id, "operationReason": reason }),
    )
    .await
}

/// 关闭工单：仅 incidentId，无 reason
pub async fn close_incident(
    client: &reqwest::Client,
    token: &str,
    incident_id: &str,
) -> Result<Value, String> {
    do_post(
        client,
        token,
        "/api/itsm/incidentAction/close-incident",
        json!({ "params": { "incidentId": incident_id } }),
    )
    .await
}

// ============ 带错误分类的列表拉取（scheduler 专用） ============

#[derive(Debug, Clone, PartialEq)]
pub enum RefreshError {
    Network,
    Auth,
    Server,
}

#[derive(Debug)]
pub enum FetchError {
    Network(String),
    Auth,
    Server(String),
}

/// 纯分类：依据 status / 是否连接超时 / 是否解析失败
pub fn classify(status_code: Option<u16>, is_connect_or_timeout: bool, parse_failed: bool) -> RefreshError {
    if matches!(status_code, Some(401)) {
        return RefreshError::Auth;
    }
    if is_connect_or_timeout {
        return RefreshError::Network;
    }
    if parse_failed {
        return RefreshError::Server;
    }
    match status_code {
        Some(c) if (500..600).contains(&c) => RefreshError::Server,
        None => RefreshError::Network,
        Some(_) => RefreshError::Server,
    }
}

pub fn map_fetch_error(e: &FetchError) -> RefreshError {
    match e {
        FetchError::Network(_) => RefreshError::Network,
        FetchError::Auth => RefreshError::Auth,
        FetchError::Server(_) => RefreshError::Server,
    }
}

/// 纯解析：list_tickets 响应 → (data 数组, count)
pub fn parse_tickets_response(v: &Value) -> Result<(Value, i64), FetchError> {
    let data = v.get("data").ok_or_else(|| FetchError::Server("响应缺 data".into()))?;
    let arr = data.get("data").cloned().unwrap_or(Value::Array(vec![]));
    let count = data.get("count").and_then(|c| c.as_i64()).unwrap_or(0);
    Ok((arr, count))
}

pub async fn fetch_tickets_raw(
    client: &reqwest::Client,
    token: &str,
    seach_type: i64,
    page_index: i64,
    page_rows: i64,
    search: Option<&SearchParams>,
) -> Result<(Value, i64), FetchError> {
    let mut params = serde_json::Map::new();
    params.insert("seachType".into(), json!(seach_type));
    merge_search(&mut params, search);
    let body = json!({
        "pageIndex": page_index, "pageRows": page_rows,
        "params": params,
        "orderByBean": { "attributeName": "", "sortType": "" }
    });
    let url = format!("{}/api/itsm/incidentService/find-pagination", API_BASE);
    let resp = client
        .post(&url)
        .header("authorization", token)
        .header("language", "zh#cn")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            if e.is_connect() || e.is_timeout() {
                FetchError::Network(e.to_string())
            } else {
                FetchError::Server(e.to_string())
            }
        })?;
    let status = resp.status().as_u16();
    if classify(Some(status), false, false) == RefreshError::Auth {
        return Err(FetchError::Auth);
    }
    if !(200..300).contains(&status) {
        return Err(FetchError::Server(format!("HTTP {}", status)));
    }
    let v: Value = resp
        .json()
        .await
        .map_err(|e| FetchError::Server(format!("解析失败: {}", e)))?;
    if is_permission_not_pass(&v) {
        return Err(FetchError::Auth);
    }
    parse_tickets_response(&v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn search_params_empty_when_all_blank() {
        assert!(SearchParams::default().is_empty());
        assert!(SearchParams {
            code_and_subject: Some("".into()),
            status: Some("".into()),
            creation_date_begin: None,
            creation_date_end: Some("".into()),
            contact_customer_group_name: Some("".into()),
        }
        .is_empty());
    }

    #[test]
    fn search_params_not_empty_when_any_filled() {
        assert!(!SearchParams { status: Some("Suspend".into()), ..Default::default() }.is_empty());
    }

    #[test]
    fn search_params_to_params_full() {
        let s = SearchParams {
            code_and_subject: Some("kw".into()),
            status: Some("Suspend".into()),
            creation_date_begin: Some("2026-01-01".into()),
            creation_date_end: Some("2026-07-22".into()),
            contact_customer_group_name: Some("cg".into()),
        };
        let v = s.to_params();
        assert_eq!(v["codeAndSubject"], json!("kw"));
        assert_eq!(v["status"], json!("Suspend"));
        assert_eq!(v["creationDateSearch"], json!(["2026-01-01", "2026-07-22"]));
        assert_eq!(v["contactCustomerGroupName"], json!("cg"));
    }

    #[test]
    fn search_params_to_params_skips_blanks_and_single_date() {
        // 单边日期不发送
        let s = SearchParams {
            creation_date_begin: Some("2026-01-01".into()),
            creation_date_end: Some("".into()),
            code_and_subject: Some("kw".into()),
            ..Default::default()
        };
        let v = s.to_params();
        assert!(v.get("creationDateSearch").is_none());
        assert_eq!(v["codeAndSubject"], json!("kw"));
    }

    #[test]
    fn search_params_to_params_empty_yields_empty_object() {
        let v = SearchParams::default().to_params();
        assert!(v.as_object().map(|m| m.is_empty()).unwrap_or(false));
    }

    #[test]
    fn classify_401_is_auth() {
        assert_eq!(classify(Some(401), false, false), RefreshError::Auth);
    }

    #[test]
    fn classify_connect_is_network() {
        assert_eq!(classify(None, true, false), RefreshError::Network);
    }

    #[test]
    fn classify_503_is_server() {
        assert_eq!(classify(Some(503), false, false), RefreshError::Server);
    }

    #[test]
    fn classify_parse_fail_is_server() {
        assert_eq!(classify(Some(200), false, true), RefreshError::Server);
    }

    #[test]
    fn classify_400_is_server() {
        assert_eq!(classify(Some(400), false, false), RefreshError::Server);
    }

    #[test]
    fn map_network() {
        assert_eq!(map_fetch_error(&FetchError::Network("x".into())), RefreshError::Network);
    }

    #[test]
    fn map_auth() {
        assert_eq!(map_fetch_error(&FetchError::Auth), RefreshError::Auth);
    }

    #[test]
    fn map_server() {
        assert_eq!(map_fetch_error(&FetchError::Server("y".into())), RefreshError::Server);
    }

    #[test]
    fn parse_normal() {
        let v = json!({"code": 800, "data": {"data": [{"id": "A"}], "count": 5}});
        let (arr, count) = parse_tickets_response(&v).unwrap();
        assert_eq!(count, 5);
        assert_eq!(arr, json!([{"id": "A"}]));
    }

    #[test]
    fn parse_missing_data_is_error() {
        let v = json!({"code": 800});
        assert!(parse_tickets_response(&v).is_err());
    }

    #[test]
    fn parse_missing_count_defaults_zero() {
        let v = json!({"data": {"data": []}});
        let (_, count) = parse_tickets_response(&v).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn parse_missing_inner_data_defaults_empty() {
        let v = json!({"data": {"count": 3}});
        let (arr, count) = parse_tickets_response(&v).unwrap();
        assert_eq!(count, 3);
        assert_eq!(arr, json!([]));
    }

    #[test]
    fn parse_upload_ok() {
        let v = json!({"code":800,"data":{"fileId":"123","filePath":"https://x/y.png","sourceFileName":"a.png"},"msg":"操作成功"});
        let r = parse_upload_response(&v).unwrap();
        assert_eq!(r.file_id, "123");
        assert_eq!(r.file_path, "https://x/y.png");
        assert_eq!(r.file_name, "a.png");
    }

    #[test]
    fn parse_upload_wrong_code_is_err() {
        let v = json!({"code":500,"msg":"失败"});
        assert!(parse_upload_response(&v).is_err());
    }

    #[test]
    fn parse_upload_missing_data_is_err() {
        let v = json!({"code":800});
        assert!(parse_upload_response(&v).is_err());
    }

    #[test]
    fn parse_upload_missing_fileid_is_err() {
        let v = json!({"code":800,"data":{"filePath":"https://x/y.png"}});
        assert!(parse_upload_response(&v).is_err());
    }

    #[test]
    fn reply_body_carries_file_ids_and_html() {
        let body = reply_body("OID", "<p>hi <strong>x</strong></p>", &["a".into(), "b".into()], false, "1");
        let p = body.get("params").unwrap();
        assert_eq!(p.get("detail").unwrap(), "<p>hi <strong>x</strong></p>");
        assert_eq!(p.get("fileIds").unwrap(), &json!(["a", "b"]));
        assert_eq!(p.get("isPrivate").unwrap(), &json!(0));
    }

    #[test]
    fn reply_body_empty_file_ids() {
        let body = reply_body("OID", "<p>x</p>", &[], true, "1");
        let p = body.get("params").unwrap();
        assert_eq!(p.get("fileIds").unwrap(), &json!([]));
        assert_eq!(p.get("isPrivate").unwrap(), &json!(1));
    }

    #[test]
    fn permission_not_pass_is_detected_for_all_http_helpers() {
        let v = json!({
            "code": "-1",
            "msgCode": "1011_common_119",
            "status": "PERMISSION_NOT_PASS"
        });
        assert!(is_permission_not_pass(&v));
        assert!(!is_permission_not_pass(&json!({"code": 800})));
    }
}
