use serde_json::{Value, json};

pub fn openapi_document() -> Value {
    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "kat-rs local API",
            "version": env!("CARGO_PKG_VERSION")
        },
        "paths": {
            "/v1/health": {
                "get": {
                    "summary": "Check local server health",
                    "responses": {
                        "200": {
                            "description": "Server is healthy"
                        }
                    }
                }
            },
            "/v1/datasources": {
                "get": {
                    "summary": "List datasources",
                    "responses": {
                        "200": {
                            "description": "Datasource list"
                        }
                    }
                },
                "post": {
                    "summary": "Create or reuse a datasource",
                    "responses": {
                        "200": {
                            "description": "Existing datasource was reused"
                        },
                        "201": {
                            "description": "Datasource was created"
                        }
                    }
                }
            },
            "/v1/datasources/{datasourceId}": {
                "get": {
                    "summary": "Get datasource metadata",
                    "responses": {
                        "200": {
                            "description": "Datasource metadata"
                        },
                        "404": {
                            "description": "Datasource not found",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "$ref": "#/components/schemas/ErrorEnvelope"
                                    }
                                }
                            }
                        }
                    }
                },
                "delete": {
                    "summary": "Delete a datasource from the local server registry",
                    "responses": {
                        "204": {
                            "description": "Datasource deleted"
                        }
                    }
                }
            },
            "/v1/datasources/{datasourceId}/queries": {
                "post": {
                    "summary": "Run SQL against a datasource",
                    "responses": {
                        "200": {
                            "description": "Query result"
                        },
                        "422": {
                            "description": "Query failed",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "$ref": "#/components/schemas/ErrorEnvelope"
                                    }
                                }
                            }
                        }
                    }
                }
            },
            "/v1/server": {
                "delete": {
                    "summary": "Request graceful local server shutdown",
                    "responses": {
                        "202": {
                            "description": "Shutdown accepted"
                        }
                    }
                }
            }
        },
        "components": {
            "schemas": {
                "ErrorEnvelope": {
                    "type": "object",
                    "required": ["error"],
                    "properties": {
                        "error": {
                            "type": "object",
                            "required": ["code", "message"],
                            "properties": {
                                "code": { "type": "string" },
                                "message": { "type": "string" },
                                "details": true
                            }
                        }
                    }
                }
            }
        }
    })
}
