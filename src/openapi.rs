//! Hand-written OpenAPI 3.1 document. `tests/api.rs` checks that every documented operation
//! exists on the router and that the router has no undocumented route.

use axum::Json;
use serde_json::{json, Value};

pub fn document() -> Value {
    let problem = json!({ "$ref": "#/components/responses/Problem" });
    let secured = json!([{ "bearer": [] }]);
    let id_param = json!([{ "name": "id", "in": "path", "required": true, "schema": { "type": "integer", "format": "int64" } }]);
    let note_body = json!({ "required": true, "content": { "application/json": { "schema": { "$ref": "#/components/schemas/NoteInput" } } } });
    let note_ok = |status: &str, desc: &str| json!({ status: { "description": desc, "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Note" } } } } });
    let creds = json!({ "required": true, "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Credentials" } } } });

    json!({
        "openapi": "3.1.0",
        "info": { "title": "rust-api-service", "version": env!("CARGO_PKG_VERSION"),
                  "description": "Notes API. Errors are application/problem+json (RFC 9457)." },
        "paths": {
            "/healthz": { "get": { "summary": "Liveness and database check",
                "responses": { "200": { "description": "Database answered" }, "503": problem } } },
            "/openapi.json": { "get": { "summary": "This document",
                "responses": { "200": { "description": "OpenAPI document" } } } },
            "/auth/register": { "post": { "summary": "Create an account", "requestBody": creds,
                "responses": { "201": { "description": "Account created" }, "409": problem, "422": problem } } },
            "/auth/login": { "post": { "summary": "Exchange credentials for a bearer token", "requestBody": creds,
                "responses": { "200": { "description": "Token issued" }, "401": problem } } },
            "/notes": {
                "get": { "summary": "List your notes, keyset-paginated", "security": secured,
                    "parameters": [
                        { "name": "limit", "in": "query", "schema": { "type": "integer", "minimum": 1, "maximum": 100, "default": 20 } },
                        { "name": "after", "in": "query", "schema": { "type": "integer", "minimum": 0 }, "description": "Value of next_after from the previous page" } ],
                    "responses": { "200": { "description": "A page of notes" }, "401": problem, "422": problem } },
                "post": { "summary": "Create a note", "security": secured, "requestBody": note_body,
                    "responses": { "201": note_ok("201", "Created")["201"].clone(), "401": problem, "422": problem } } },
            "/notes/{id}": {
                "get": { "summary": "Read one of your notes", "security": secured, "parameters": id_param,
                    "responses": { "200": note_ok("200", "The note")["200"].clone(), "401": problem, "404": problem } },
                "put": { "summary": "Replace one of your notes", "security": secured, "parameters": id_param, "requestBody": note_body,
                    "responses": { "200": note_ok("200", "Updated")["200"].clone(), "401": problem, "404": problem, "422": problem } },
                "delete": { "summary": "Delete one of your notes", "security": secured, "parameters": id_param,
                    "responses": { "204": { "description": "Deleted" }, "401": problem, "404": problem } } }
        },
        "components": {
            "securitySchemes": { "bearer": { "type": "http", "scheme": "bearer" } },
            "responses": { "Problem": { "description": "RFC 9457 problem details",
                "content": { "application/problem+json": { "schema": { "$ref": "#/components/schemas/Problem" } } } } },
            "schemas": {
                "Credentials": { "type": "object", "required": ["email", "password"], "additionalProperties": false,
                    "properties": { "email": { "type": "string" }, "password": { "type": "string", "minLength": 8, "maxLength": 128 } } },
                "NoteInput": { "type": "object", "required": ["title"], "additionalProperties": false,
                    "properties": { "title": { "type": "string", "minLength": 1, "maxLength": 200 }, "body": { "type": "string", "maxLength": 10000 } } },
                "Note": { "type": "object", "required": ["id", "title", "body", "created_at", "updated_at"],
                    "properties": { "id": { "type": "integer" }, "title": { "type": "string" }, "body": { "type": "string" },
                                    "created_at": { "type": "integer" }, "updated_at": { "type": "integer" } } },
                "Problem": { "type": "object", "required": ["type", "title", "status", "detail"],
                    "properties": { "type": { "type": "string" }, "title": { "type": "string" }, "status": { "type": "integer" },
                                    "detail": { "type": "string" },
                                    "errors": { "type": "array", "items": { "type": "object",
                                        "properties": { "field": { "type": "string" }, "message": { "type": "string" } } } } } }
            }
        }
    })
}

pub async fn spec() -> Json<Value> {
    Json(document())
}
