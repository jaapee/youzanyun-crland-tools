use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const CLIENT_ID: &str = "ee445730e670364ae0";
const CLIENT_SECRET: &str = "c6642ef7936f94c53d1bbf5635803acb";
const GRANT_ID: &str = "180198027";
const MIXC_URL: &str = "http://ztopenapiuat.crland.com.cn:81/api-gateway/rs-service/";
const MIXC_API_ID: &str = "mixc.imPOSWBJB.GLWXCJB.orderCollect";
const MIXC_VERSION: &str = "1.0.0";
const MIXC_APP_SUB_ID: &str = "10000133334PY";
const MIXC_APP_TOKEN: &str = "c861e8a4be0f41b182abdb55b986444a";
const MIXC_APP_PUB_ID: &str = "10000133301US";
const MIXC_PARTNER_ID: &str = "70000006";
const MIXC_SYS_ID: &str = "100001333";
const MIXC_SIGN_KEY: &str = "0bca40d57d1f44208d787a4e0a87957d";
const PAUSE_YOUZAN_SYNC: bool = false;
const PAUSE_MIXC_PUSH: bool = false;
const MIXC_TEST_TID: &str = "E20260902115917052500001";
#[derive(Debug, Serialize, Deserialize)]
pub struct Order {
    pub tid: String,
    pub status: Option<String>,
    pub status_str: Option<String>,
    pub payment: Option<f64>,
    pub created: Option<String>,
    pub receiver_name: Option<String>,
    pub receiver_phone: Option<String>,
    pub mixc_order_id: Option<String>,
    pub mixc_refund_order_id: Option<String>,
}
fn text(v: Option<&serde_json::Value>) -> Option<String> {
    v.and_then(|v| {
        if v.is_string() {
            v.as_str().map(str::to_owned)
        } else if v.is_null() {
            None
        } else {
            Some(v.to_string())
        }
    })
}
fn number(v: Option<&serde_json::Value>) -> Option<f64> {
    v.and_then(|v| v.as_f64().or_else(|| v.as_str()?.parse().ok()))
}
fn integer(v: Option<&serde_json::Value>) -> Option<i64> {
    number(v).map(|n| n as i64)
}
fn db_path() -> Result<PathBuf, String> {
    dirs::data_local_dir()
        .ok_or("无法找到本地数据目录".into())
        .map(|p| p.join("youzan-order-sync.sqlite3"))
}
fn connection() -> Result<Connection, String> {
    let path = db_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let c = Connection::open(path).map_err(|e| e.to_string())?;
    c.execute_batch("CREATE TABLE IF NOT EXISTS orders (tid TEXT PRIMARY KEY, status TEXT, type INTEGER, status_str TEXT, pay_type INTEGER, team_type INTEGER, close_type INTEGER, created TEXT, update_time TEXT, expired_time TEXT, pay_time TEXT, refund_state INTEGER, success_time TEXT, pay_type_str TEXT, pay_type_desc TEXT, shop_name TEXT, buyer_info TEXT, buyer_phone TEXT, payment REAL, total_fee REAL, receiver_name TEXT, receiver_phone TEXT, mixc_sent_amount REAL); CREATE TABLE IF NOT EXISTS order_items (oid TEXT PRIMARY KEY, tid TEXT NOT NULL, item_type INTEGER, title TEXT, num INTEGER, buyer_messages TEXT, price REAL, total_fee REAL, payment REAL, item_id INTEGER, sku_id INTEGER); CREATE TABLE IF NOT EXISTS app_state (key TEXT PRIMARY KEY, value TEXT); CREATE INDEX IF NOT EXISTS idx_order_items_tid ON order_items(tid);").map_err(|e| e.to_string())?;
    for (name, kind) in [
        ("type", "INTEGER"),
        ("status_str", "TEXT"),
        ("pay_type", "INTEGER"),
        ("team_type", "INTEGER"),
        ("close_type", "INTEGER"),
        ("update_time", "TEXT"),
        ("expired_time", "TEXT"),
        ("pay_time", "TEXT"),
        ("refund_state", "INTEGER"),
        ("success_time", "TEXT"),
        ("pay_type_str", "TEXT"),
        ("pay_type_desc", "TEXT"),
        ("shop_name", "TEXT"),
        ("buyer_info", "TEXT"),
        ("buyer_phone", "TEXT"),
        ("total_fee", "REAL"),
        ("mixc_sent_amount", "REAL"),
        ("is_payed", "INTEGER"),
        ("is_refund", "INTEGER"),
        ("mixc_order_id", "TEXT"),
        ("mixc_push_success", "INTEGER"),
        ("mixc_request", "TEXT"),
        ("mixc_response", "TEXT"),
        ("mixc_push_attempts", "INTEGER NOT NULL DEFAULT 0"),
        ("mixc_refund_order_id", "TEXT"),
        ("mixc_refund_push_success", "INTEGER"),
        ("mixc_refund_push_attempts", "INTEGER NOT NULL DEFAULT 0"),
        ("mixc_refund_request", "TEXT"),
        ("mixc_refund_response", "TEXT"),
    ] {
        let exists: bool = c
            .prepare("SELECT 1 FROM pragma_table_info('orders') WHERE name=?1")
            .and_then(|mut s| s.exists([name]))
            .unwrap_or(false);
        if !exists {
            c.execute(&format!("ALTER TABLE orders ADD COLUMN {name} {kind}"), [])
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(c)
}
mod commands {
    use super::*;
    fn order_list(value: &serde_json::Value) -> Vec<serde_json::Value> {
        if let Some(items) = value.as_array() {
            if items.iter().any(|item| {
                item.get("full_order_info").is_some() || item.get("order_info").is_some()
            }) {
                return items.clone();
            }
            for item in items {
                let found = order_list(item);
                if !found.is_empty() {
                    return found;
                }
            }
        } else if let Some(object) = value.as_object() {
            for key in ["trades", "data", "orders"] {
                if let Some(found) = object
                    .get(key)
                    .map(order_list)
                    .filter(|items| !items.is_empty())
                {
                    return found;
                }
            }
            for child in object.values() {
                let found = order_list(child);
                if !found.is_empty() {
                    return found;
                }
            }
        }
        Vec::new()
    }
    fn save_order(c: &Connection, raw: &serde_json::Value) -> Result<(), String> {
        let full = raw.get("full_order_info").unwrap_or(raw);
        let info = full.get("order_info").unwrap_or(full);
        let tid = text(info.get("tid")).unwrap_or_default();
        if tid.is_empty() {
            return Ok(());
        }
        let buyer = full.get("buyer_info");
        let address = full.get("address_info");
        let pay = full.get("pay_info");
        let tags = info.get("order_tags").or_else(|| full.get("order_tags"));
        let is_payed = flag(tags, "is_payed") as i64;
        let is_refund = flag(tags, "is_refund") as i64;
        c.execute("INSERT INTO orders (tid,status,type,status_str,pay_type,team_type,close_type,created,update_time,expired_time,pay_time,refund_state,success_time,pay_type_str,pay_type_desc,shop_name,buyer_info,buyer_phone,payment,total_fee,receiver_name,receiver_phone,is_payed,is_refund) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24) ON CONFLICT(tid) DO UPDATE SET status=excluded.status,type=excluded.type,status_str=excluded.status_str,pay_type=excluded.pay_type,team_type=excluded.team_type,close_type=excluded.close_type,created=excluded.created,update_time=excluded.update_time,expired_time=excluded.expired_time,pay_time=excluded.pay_time,refund_state=excluded.refund_state,success_time=excluded.success_time,pay_type_str=excluded.pay_type_str,pay_type_desc=excluded.pay_type_desc,shop_name=excluded.shop_name,buyer_info=excluded.buyer_info,buyer_phone=excluded.buyer_phone,payment=excluded.payment,total_fee=excluded.total_fee,receiver_name=excluded.receiver_name,receiver_phone=excluded.receiver_phone,is_payed=excluded.is_payed,is_refund=excluded.is_refund", params![tid,text(info.get("status")),integer(info.get("type")),text(info.get("status_str")),integer(info.get("pay_type")),integer(info.get("team_type")),integer(info.get("close_type")),text(info.get("created")),text(info.get("update_time")),text(info.get("expired_time")),text(info.get("pay_time")),integer(info.get("refund_state")),text(info.get("success_time")),text(info.get("pay_type_str")),text(info.get("pay_type_desc")),text(info.get("shop_name")),buyer.map(|v| v.to_string()),text(buyer.and_then(|v| v.get("buyer_phone"))),number(pay.and_then(|v| v.get("payment"))),number(pay.and_then(|v| v.get("total_fee"))),text(address.and_then(|v| v.get("receiver_name"))),text(address.and_then(|v| v.get("receiver_tel"))),is_payed,is_refund ]).map_err(|e| e.to_string())?;
        if let Some(items) = full.get("orders").and_then(|v| v.as_array()) {
            c.execute("DELETE FROM order_items WHERE tid=?1", params![tid])
                .map_err(|e| e.to_string())?;
            for item in items {
                let oid = text(item.get("oid")).unwrap_or_default();
                if oid.is_empty() {
                    continue;
                }
                c.execute("INSERT INTO order_items (oid,tid,item_type,title,num,buyer_messages,price,total_fee,payment,item_id,sku_id) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11) ON CONFLICT(oid) DO UPDATE SET tid=excluded.tid,item_type=excluded.item_type,title=excluded.title,num=excluded.num,buyer_messages=excluded.buyer_messages,price=excluded.price,total_fee=excluded.total_fee,payment=excluded.payment,item_id=excluded.item_id,sku_id=excluded.sku_id", params![oid,tid,integer(item.get("item_type")),text(item.get("title")),integer(item.get("num")),text(item.get("buyer_messages")),number(item.get("price")),number(item.get("total_fee")),number(item.get("payment")),integer(item.get("item_id")),integer(item.get("sku_id"))]).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
    fn flag(value: Option<&serde_json::Value>, key: &str) -> bool {
        let Some(value) = value else {
            return false;
        };
        if let Some(v) = value.get(key) {
            return v.as_bool().unwrap_or_else(|| {
                v.as_i64().unwrap_or(0) != 0
                    || v.as_str()
                        .map(|s| s == "true" || s == "1" || s == "已支付" || s == "退款")
                        .unwrap_or(false)
            });
        }
        value
            .as_array()
            .map(|items| items.iter().any(|item| flag(Some(item), key)))
            .unwrap_or(false)
    }
    fn save_error(message: &str) {
        if let Ok(c) = connection() {
            let _ = c.execute("INSERT INTO app_state(key,value) VALUES('last_sync_error',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![message]);
        }
    }
    fn clear_error() {
        if let Ok(c) = connection() {
            let _ = c.execute("DELETE FROM app_state WHERE key='last_sync_error'", []);
        }
    }
    #[tauri::command]
    pub fn last_sync_error() -> Result<Option<String>, String> {
        let c = connection()?;
        c.query_row(
            "SELECT value FROM app_state WHERE key='last_sync_error'",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())
    }
    async fn push_order(
        client: &reqwest::Client,
        raw: &serde_json::Value,
        mixc_order_id: Option<String>,
        refund_order_id: Option<String>,
        attempts: i64,
    ) -> Result<Option<(String, f64, String, String, String)>, (String, String)> {
        let full = raw.get("full_order_info").unwrap_or(raw);
        let info = full.get("order_info").unwrap_or(full);
        let tags = info.get("order_tags").or_else(|| full.get("order_tags"));
        let refunded =
            flag(tags, "is_refund") || integer(info.get("refund_state")).unwrap_or(0) > 0;
        if refunded && mixc_order_id.is_none() {
            println!("[万象城] 跳过退款单：对应销售单尚未成功推送");
            return Ok(None);
        }
        if (refunded && refund_order_id.is_some())
            || (!refunded && mixc_order_id.is_some())
            || attempts >= 3
        {
            println!("[万象城] 跳过订单：已成功推送或失败次数已达 3 次");
            return Ok(None);
        }
        println!(
            "[万象城] 准备检查订单={} is_payed={} is_refund={} attempts={}",
            text(info.get("tid")).unwrap_or_default(),
            flag(tags, "is_payed"),
            flag(tags, "is_refund"),
            attempts
        );
        if !flag(tags, "is_payed") {
            println!("[万象城] 跳过订单：未支付");
            return Ok(None);
        }
        let tid = text(info.get("tid")).unwrap_or_default();
        if tid.is_empty() {
            return Ok(None);
        }
        let amount = number(full.get("pay_info").and_then(|v| v.get("payment")))
            .filter(|amount| *amount > 0.0)
            .or_else(|| number(info.get("payment")).filter(|amount| *amount > 0.0))
            .or_else(|| number(info.get("total_fee")))
            .unwrap_or(0.0);
        let target = if refunded {
            -amount.abs()
        } else {
            amount.abs()
        };
        if amount <= 0.0 {
            println!(
                "[万象城] 跳过订单：金额为 0，amount={} target={}",
                amount, target
            );
            return Ok(None);
        }
        let now = chrono::Local::now().format("%Y%m%d%H%M%S").to_string();
        let order_id = if refunded {
            format!("{}R", tid)
        } else {
            tid.clone()
        };
        let mut request_data = serde_json::json!({"cashierId":"20028hvgl120n0101","checkCode":"p88888888","itemList":[],"mall":"20028","orderId":order_id,"payList":[{"discountAmt":target,"payAmt":target,"paymentMethod":"CH","time":now,"value":target}],"source":"01","store":"HVGL120N01","tillId":"01","time":now,"totalAmt":target,"type":if refunded {"ONLINEREFUND"} else {"SALE"}});
        if refunded {
            request_data["refOrderId"] = serde_json::Value::String(tid.clone());
        }
        let attrs = serde_json::json!({"Api_ID":MIXC_API_ID,"Api_Version":MIXC_VERSION,"App_Pub_ID":MIXC_APP_PUB_ID,"App_Sub_ID":MIXC_APP_SUB_ID,"App_Token":MIXC_APP_TOKEN,"Format":"json","Partner_ID":MIXC_PARTNER_ID,"Sign_Method":"md5","Sys_ID":MIXC_SYS_ID,"Time_Stamp":chrono::Local::now().format("%Y-%m-%d %H:%M:%S:%3f").to_string()});
        let mut pairs: Vec<(String, String)> = attrs
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
            .collect();
        pairs.push(("REQUEST_DATA".into(), request_data.to_string()));
        pairs.sort_by(|a, b| a.0.cmp(&b.0));
        let signing = pairs
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("&")
            + "&"
            + MIXC_SIGN_KEY;
        let sign = format!("{:X}", md5::compute(signing));
        let mut attrs_obj = attrs.as_object().unwrap().clone();
        attrs_obj.insert("Sign".into(), serde_json::Value::String(sign));
        let body =
            serde_json::json!({"REQUEST":{"REQUEST_DATA":request_data,"HRT_ATTRS":attrs_obj}});
        let request_text = body.to_string();
        println!(
            "[万象城] POST {} order_id={} amount={} type={} request={}",
            MIXC_URL,
            tid,
            target,
            if refunded { "ONLINEREFUND" } else { "SALE" },
            body
        );
        let response_text = client
            .post(MIXC_URL)
            .header("Content-Type", "application/json;charset=UTF-8")
            .json(&body)
            .send()
            .await
            .map_err(|e| (request_text.clone(), e.to_string()))?
            .text()
            .await
            .map_err(|e| (request_text.clone(), e.to_string()))?;
        let response: serde_json::Value = serde_json::from_str(&response_text).map_err(|e| {
            (
                request_text.clone(),
                format!("{}; response={}", e, response_text),
            )
        })?;
        println!("[万象城] order_id={} response={}", tid, response);
        let mixc_order_id = response
            .pointer("/RETURN_DATA/body/orderId")
            .and_then(|v| v.as_str())
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| {
                (
                    request_text.clone(),
                    format!("万象城接口失败: {}", response),
                )
            })?;
        Ok(Some((
            tid,
            target,
            mixc_order_id,
            request_text,
            response.to_string(),
        )))
    }
    async fn get_token() -> Result<String, String> {
        let body = serde_json::json!({"authorize_type":"silent","client_id":CLIENT_ID,"client_secret":CLIENT_SECRET,"grant_id":GRANT_ID,"refresh":false});
        println!(
            "[Youzan] POST /auth/token params: client_id={}, grant_id={}, refresh=false",
            CLIENT_ID, GRANT_ID
        );
        let value: serde_json::Value = reqwest::Client::new()
            .post("https://open.youzanyun.com/auth/token")
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        println!("[Youzan] token response: {}", value);
        value
            .pointer("/data/access_token")
            .or_else(|| value.pointer("/data/data/access_token"))
            .and_then(|v| v.as_str())
            .map(str::to_owned)
            .ok_or_else(|| value.to_string())
    }
    #[tauri::command]
    pub fn list_orders(
        page: Option<i64>,
        page_size: Option<i64>,
        search: Option<String>,
    ) -> Result<Vec<Order>, String> {
        let page = page.unwrap_or(1).max(1);
        let page_size = page_size.unwrap_or(20).clamp(1, 100);
        let offset = (page - 1) * page_size;
        let c = connection()?;
        let search = search.unwrap_or_default();
        let pattern = format!("%{}%", search);
        let mut s = c.prepare("SELECT tid,status,status_str,payment,created,receiver_name,receiver_phone,mixc_order_id,mixc_refund_order_id FROM orders WHERE ?1='' OR tid LIKE ?2 OR mixc_order_id LIKE ?2 OR mixc_refund_order_id LIKE ?2 ORDER BY created DESC LIMIT ?3 OFFSET ?4").map_err(|e| e.to_string())?;
        let rows = s
            .query_map(params![search, pattern, page_size, offset], |r| {
                Ok(Order {
                    tid: r.get(0)?,
                    status: r.get(1)?,
                    status_str: r.get(2)?,
                    payment: r.get(3)?,
                    created: r.get(4)?,
                    receiver_name: r.get(5)?,
                    receiver_phone: r.get(6)?,
                    mixc_order_id: r.get(7)?,
                    mixc_refund_order_id: r.get(8)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.map(|r| r.map_err(|e| e.to_string())).collect()
    }
    #[tauri::command]
    pub fn count_orders() -> Result<i64, String> {
        let c = connection()?;
        c.query_row("SELECT COUNT(*) FROM orders", [], |row| row.get(0))
            .map_err(|e| e.to_string())
    }
    #[tauri::command]
    pub async fn sync_orders(access_token: String, _kdt_id: i64) -> Result<Vec<Order>, String> {
        if access_token.trim().is_empty() {
            return Err("请先配置 access_token".into());
        }
        let body = serde_json::json!({"page_no":1,"page_size":100,"start_created":"2020-01-01 00:00:00","end_created":"2099-01-01 00:00:00"});
        let response: serde_json::Value = reqwest::Client::new()
            .post("https://open.youzanyun.com/api/youzan.trades.sold.get/4.0.4")
            .query(&[("access_token", access_token)])
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        let list = order_list(&response);
        let c = connection()?;
        for item in &list {
            save_order(&c, item)?;
        }
        list_orders(Some(1), Some(20), None)
    }
    #[tauri::command]
    pub async fn refresh_token(refresh: bool) -> Result<String, String> {
        let body = serde_json::json!({"authorize_type":"silent","client_id":CLIENT_ID,"client_secret":CLIENT_SECRET,"grant_id":GRANT_ID,"refresh":refresh});
        let value: serde_json::Value = reqwest::Client::new()
            .post("https://open.youzanyun.com/auth/token")
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        value
            .pointer("/data/access_token")
            .and_then(|v| v.as_str())
            .or_else(|| {
                value
                    .pointer("/data/data/access_token")
                    .and_then(|v| v.as_str())
            })
            .map(str::to_owned)
            .ok_or_else(|| value.to_string())
    }
    #[tauri::command]
    pub async fn sync_recent_orders() -> Result<Vec<Order>, String> {
        clear_error();
        if PAUSE_MIXC_PUSH {
            println!("[同步] 武汉万象城推送已暂停");
        }
        if PAUSE_YOUZAN_SYNC {
            println!("[同步] 已暂停有赞拉取，开始测试万象城推送");
            let c = connection()?;
            let row = c
                .query_row("SELECT tid,is_payed,is_refund,payment,total_fee,mixc_order_id,mixc_refund_order_id,mixc_refund_push_attempts FROM orders WHERE tid=?1", params![MIXC_TEST_TID], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, Option<f64>>(3)?, r.get::<_, Option<f64>>(4)?, r.get::<_, Option<String>>(5)?, r.get::<_, Option<String>>(6)?, r.get::<_, Option<i64>>(7)?.unwrap_or(0)))
                })
                .optional()
                .map_err(|e| e.to_string())?;
            drop(c);
            if let Some((
                tid,
                is_payed,
                is_refund,
                payment,
                total_fee,
                mut mixc_id,
                refund_id,
                attempts,
            )) = row
            {
                println!("[同步] 测试订单={} is_payed={} is_refund={} payment={:?} total_fee={:?} mixc_id={:?} attempts={}", tid, is_payed, is_refund, payment, total_fee, mixc_id, attempts);
                let amount = payment
                    .filter(|amount| *amount > 0.0)
                    .or(total_fee)
                    .unwrap_or(0.0);
                let raw = serde_json::json!({"order_info":{"tid":tid,"order_tags":{"is_payed":is_payed == 1,"is_refund":is_refund == 1}},"pay_info":{"payment":amount}});
                let client = reqwest::Client::new();
                if is_refund == 1 && mixc_id.is_none() {
                    println!("[同步] 退款测试先推送对应销售单");
                    let sale_raw = serde_json::json!({"order_info":{"tid":tid,"order_tags":{"is_payed":true,"is_refund":false}},"pay_info":{"payment":amount}});
                    if let Ok(Some((sale_tid, sale_amount, sale_id, request, response))) =
                        push_order(&client, &sale_raw, None, None, 0).await
                    {
                        connection()?.execute("UPDATE orders SET mixc_sent_amount=?1,mixc_order_id=?2,mixc_push_success=1,mixc_request=?3,mixc_response=?4 WHERE tid=?5", params![sale_amount, sale_id, request, response, sale_tid]).map_err(|e| e.to_string())?;
                        mixc_id = Some(sale_id);
                    } else {
                        println!("[同步] 对应销售单推送失败，暂不测试退款单");
                        return list_orders(Some(1), Some(20), None);
                    }
                }
                if mixc_id.is_some() && refund_id.is_none() && attempts < 3 {
                    connection()?.execute("UPDATE orders SET mixc_refund_push_attempts=mixc_refund_push_attempts+1 WHERE tid=?1", params![tid]).map_err(|e| e.to_string())?;
                }
                match push_order(&client, &raw, mixc_id, refund_id, attempts).await {
                    Ok(Some((tid, _amount, mixc_id, request, response))) => {
                        connection()?.execute("UPDATE orders SET mixc_refund_order_id=?1,mixc_refund_push_success=1,mixc_refund_request=?2,mixc_refund_response=?3 WHERE tid=?4", params![mixc_id, request, response, tid]).map_err(|e| e.to_string())?;
                    }
                    Err((request, error)) => {
                        connection()?.execute("UPDATE orders SET mixc_refund_push_success=0,mixc_refund_request=?1,mixc_refund_response=?2 WHERE tid=?3", params![request, error, tid]).map_err(|e| e.to_string())?;
                    }
                    Ok(None) => {}
                }
            } else {
                println!("[同步] 未找到测试订单 {}", MIXC_TEST_TID);
            }
            return list_orders(Some(1), Some(20), None);
        }
        let token = get_token().await?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_secs() as i64;
        let end = chrono::DateTime::from_timestamp(now, 0)
            .ok_or("时间错误")?
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let start = chrono::DateTime::from_timestamp(now - 48 * 3600, 0)
            .ok_or("时间错误")?
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let client = reqwest::Client::new();
        let mut page = 1;
        loop {
            let body = serde_json::json!({"page_no":page,"page_size":100,"start_update":start,"end_update":end});
            println!(
                "[Youzan] POST /youzan.trades.sold.get/4.0.4 page={} params: {}",
                page, body
            );
            let value: serde_json::Value = client
                .post("https://open.youzanyun.com/api/youzan.trades.sold.get/4.0.4")
                .query(&[("access_token", &token)])
                .json(&body)
                .send()
                .await
                .map_err(|e| {
                    save_error(&e.to_string());
                    e.to_string()
                })?
                .json()
                .await
                .map_err(|e| {
                    save_error(&e.to_string());
                    e.to_string()
                })?;
            println!("[Youzan] page={} response: {}", page, value);
            if let Some(err) = value.get("gw_err_resp") {
                let message = err
                    .get("err_msg")
                    .and_then(|v| v.as_str())
                    .unwrap_or_else(|| {
                        err.get("err_code")
                            .and_then(|v| v.as_str())
                            .unwrap_or("有赞接口返回错误")
                    });
                save_error(message);
                return Err(message.to_string());
            }
            let list = order_list(&value);
            let count = list.len();
            for item in list {
                let c = connection()?;
                save_order(&c, &item)?;
                let full = item.get("full_order_info").unwrap_or(&item);
                let info = full.get("order_info").unwrap_or(full);
                let tid = text(info.get("tid")).unwrap_or_default();
                if !PAUSE_MIXC_PUSH {
                    let (mixc_id, refund_id, sale_attempts, refund_attempts): (Option<String>, Option<String>, i64, i64) = c.query_row("SELECT mixc_order_id,mixc_refund_order_id,COALESCE(mixc_push_attempts,0),COALESCE(mixc_refund_push_attempts,0) FROM orders WHERE tid=?1", params![tid], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(|e| e.to_string())?;
                    let tags = info.get("order_tags").or_else(|| full.get("order_tags"));
                    let refunded = flag(tags, "is_refund")
                        || integer(info.get("refund_state")).unwrap_or(0) > 0;
                    let attempts = if refunded {
                        refund_attempts
                    } else {
                        sale_attempts
                    };
                    drop(c);
                    if attempts < 3
                        && ((!refunded && mixc_id.is_none())
                            || (refunded && mixc_id.is_some() && refund_id.is_none()))
                    {
                        let column = if refunded {
                            "mixc_refund_push_attempts"
                        } else {
                            "mixc_push_attempts"
                        };
                        connection()?
                            .execute(
                                &format!("UPDATE orders SET {column}={column}+1 WHERE tid=?1"),
                                params![tid],
                            )
                            .map_err(|e| e.to_string())?;
                    }
                    match push_order(&client, &item, mixc_id, refund_id, attempts).await {
                        Err((request, error)) => {
                            let sql = if refunded {
                                "UPDATE orders SET mixc_refund_push_success=0,mixc_refund_request=?1,mixc_refund_response=?2 WHERE tid=?3"
                            } else {
                                "UPDATE orders SET mixc_push_success=0,mixc_request=?1,mixc_response=?2 WHERE tid=?3"
                            };
                            connection()?
                                .execute(sql, params![request, error, tid])
                                .map_err(|e| e.to_string())?;
                            println!("[万象城] 订单推送失败，继续处理后续订单");
                        }
                        Ok(Some((tid, amount, mixc_id, request, response))) => {
                            let sql = if refunded {
                                "UPDATE orders SET mixc_refund_order_id=?1,mixc_refund_push_success=1,mixc_refund_request=?2,mixc_refund_response=?3 WHERE tid=?4"
                            } else {
                                "UPDATE orders SET mixc_sent_amount=?1,mixc_order_id=?2,mixc_push_success=1,mixc_request=?3,mixc_response=?4 WHERE tid=?5"
                            };
                            if refunded {
                                connection()?
                                    .execute(sql, params![mixc_id, request, response, tid])
                                    .map_err(|e| e.to_string())?;
                            } else {
                                connection()?
                                    .execute(sql, params![amount, mixc_id, request, response, tid])
                                    .map_err(|e| e.to_string())?;
                            }
                        }
                        Ok(None) => {}
                    }
                }
            }
            if count < 100 {
                break;
            }
            page += 1;
        }
        list_orders(Some(1), Some(20), None)
    }
}
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::list_orders,
            commands::count_orders,
            commands::sync_orders,
            commands::sync_recent_orders,
            commands::last_sync_error,
            commands::refresh_token
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
