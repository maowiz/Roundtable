//! Repository operations used by the engine and IPC. Grows with each step of the plan.
//! Step 1 covers settings, panels and run events; run/call/paper writes arrive with the engine.

use crate::{now, Ledger, LedgerError, Result};
use rt_core::{PanelId, RunId};
use rusqlite::{params, OptionalExtension};
use serde_json::Value;

impl Ledger {
    pub fn get_setting(&self, key: &str) -> Result<Option<Value>> {
        let conn = self.writer()?;
        let s: Option<String> = conn.query_row("SELECT json FROM settings WHERE key = ?1", params![key], |r| r.get(0)).optional()?;
        Ok(match s {
            Some(s) => Some(serde_json::from_str(&s)?),
            None => None,
        })
    }

    pub fn set_setting(&self, key: &str, value: &Value) -> Result<()> {
        let conn = self.writer()?;
        conn.execute(
            "INSERT INTO settings(key, json, updated_at) VALUES(?1, ?2, ?3) ON CONFLICT(key) DO UPDATE SET json = excluded.json, updated_at = excluded.updated_at",
            params![key, serde_json::to_string(value)?, now()],
        )?;
        Ok(())
    }

    pub fn upsert_panel(&self, id: PanelId, name: &str, json: &Value, rubric_version: &str) -> Result<()> {
        let conn = self.writer()?;
        conn.execute(
            "INSERT INTO panels(id, name, json, prompt_rubric_version, created_at, updated_at) VALUES(?1, ?2, ?3, ?4, ?5, ?5)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, json = excluded.json, prompt_rubric_version = excluded.prompt_rubric_version, updated_at = excluded.updated_at",
            params![id.to_string(), name, serde_json::to_string(json)?, rubric_version, now()],
        )?;
        Ok(())
    }

    pub fn list_panels(&self) -> Result<Vec<(PanelId, String, Value)>> {
        let conn = self.writer()?;
        let mut stmt = conn.prepare("SELECT id, name, json FROM panels ORDER BY updated_at DESC")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?;
        let mut out = Vec::new();
        for row in rows {
            let (id, name, json) = row?;
            let id: PanelId = id.parse().map_err(|_| LedgerError::Corrupt(format!("bad panel id {id}")))?;
            out.push((id, name, serde_json::from_str(&json)?));
        }
        Ok(out)
    }

    pub fn delete_panel(&self, id: PanelId) -> Result<bool> {
        let conn = self.writer()?;
        Ok(conn.execute("DELETE FROM panels WHERE id = ?1", params![id.to_string()])? == 1)
    }

    /// Append an event with the next sequence number for the run. Must be called inside the
    /// same transaction as the state change it describes; this helper is for the writer connection.
    pub fn append_event(conn: &rusqlite::Connection, run: RunId, kind: &str, payload: &Value) -> Result<i64> {
        let seq: i64 = conn.query_row("SELECT COALESCE(MAX(seq), 0) + 1 FROM run_events WHERE run_id = ?1", params![run.to_string()], |r| r.get(0))?;
        conn.execute(
            "INSERT INTO run_events(run_id, seq, type, payload_json, created_at) VALUES(?1, ?2, ?3, ?4, ?5)",
            params![run.to_string(), seq, kind, serde_json::to_string(payload)?, now()],
        )?;
        Ok(seq)
    }

    pub fn events_since(&self, run: RunId, after_seq: i64, limit: usize) -> Result<Vec<(i64, String, Value)>> {
        let conn = self.writer()?;
        let mut stmt = conn.prepare("SELECT seq, type, payload_json FROM run_events WHERE run_id = ?1 AND seq > ?2 ORDER BY seq LIMIT ?3")?;
        let rows = stmt.query_map(params![run.to_string(), after_seq, limit as i64], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?;
        let mut out = Vec::new();
        for row in rows {
            let (seq, kind, payload) = row?;
            out.push((seq, kind, serde_json::from_str(&payload)?));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn settings_and_panels_round_trip() {
        let l = Ledger::in_memory().unwrap();
        assert!(l.get_setting("theme").unwrap().is_none());
        l.set_setting("theme", &json!("dark")).unwrap();
        l.set_setting("theme", &json!("light")).unwrap();
        assert_eq!(l.get_setting("theme").unwrap(), Some(json!("light")));

        let id = PanelId::new();
        l.upsert_panel(id, "Everyday thinking", &json!({"seats": 4}), "rubric.v1").unwrap();
        l.upsert_panel(id, "Everyday", &json!({"seats": 5}), "rubric.v1").unwrap();
        let panels = l.list_panels().unwrap();
        assert_eq!(panels.len(), 1);
        assert_eq!(panels[0].1, "Everyday");
        assert_eq!(panels[0].2["seats"], 5);
        assert!(l.delete_panel(id).unwrap());
        assert!(!l.delete_panel(id).unwrap());
    }

    #[test]
    fn events_are_sequenced_per_run_and_replayable_from_a_cursor() {
        let l = Ledger::in_memory().unwrap();
        let run = RunId::new();
        {
            let c = l.writer().unwrap();
            c.execute(
                "INSERT INTO runs(id,state,stage,question,panel_snapshot_json,limits_json,price_catalog_version,schema_version,app_version,adapter_versions_json,seed,created_at)
                 VALUES(?1,'draft','answer','q','{}','{}','p','1','0.1','{}','s',?2)",
                params![run.to_string(), now()],
            )
            .unwrap();
            assert_eq!(Ledger::append_event(&c, run, "run.state_changed", &json!({"state":"validating"})).unwrap(), 1);
            assert_eq!(Ledger::append_event(&c, run, "stage.started", &json!({"stage":"answer"})).unwrap(), 2);
            assert_eq!(Ledger::append_event(&c, run, "call.queued", &json!({"seat":0})).unwrap(), 3);
        }
        let tail = l.events_since(run, 1, 10).unwrap();
        assert_eq!(tail.len(), 2);
        assert_eq!(tail[0].0, 2);
        assert_eq!(tail[1].1, "call.queued");
    }
}
