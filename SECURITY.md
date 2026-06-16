# FileUni Plugin System v2 - Security Design

This document describes the security architecture and best practices for the FileUni Plugin System v2.

## Overview

The plugin system operates on the principle of **least privilege** with **defense in depth**. Plugins are untrusted code that can only access system capabilities through strictly controlled Host APIs with comprehensive permission checking.

## 1. Permission System

### 1.1 Permission Model

The system defines 17 granular permissions that plugins must explicitly declare in `plugin.json`:

| Permission Key | Scope | Description |
|----------------|-------|-------------|
| `auth-read` | Identity | Read current user identity |
| `user-lookup` | User Management | Look up user by ID |
| `user-permission-check` | Authorization | Check user permissions |
| `vfs-read` | File System | Read files through VFS |
| `vfs-write` | File System | Write files through VFS |
| `kv-read` | Key-Value Store | Read KV pairs |
| `kv-write` | Key-Value Store | Write KV pairs |
| `kv-delete` | Key-Value Store | Delete KV pairs |
| `db-shared-read` | Database | Read shared records |
| `db-shared-write` | Database | Write shared records |
| `db-sqlite` | Database | Access plugin-specific SQLite DB |
| `web-api` | Network | Expose HTTP API endpoints |
| `web-socket` | Network | Use WebSocket connections |
| `scheduler` | Tasks | Schedule background tasks |
| `network` | Network | Make outbound network requests |
| `process-execution` | Runtime | Execute as native process |
| `docker-execution` | Runtime | Execute in Docker container |

### 1.2 Permission Declaration

Plugins declare required permissions in `plugin.json`:

```json
{
  "id": "com.example.myplugin",
  "permissions": [
    "auth-read",
    "kv-read",
    "kv-write",
    "vfs-read"
  ]
}
```

### 1.3 Permission Enforcement

**All Host API endpoints enforce permissions**. When a plugin calls a Host API:

1. Extract plugin ID from the request context
2. Query granted permissions from database (`yh_plg_permission_grants`)
3. Check if the required permission is granted
4. Return `403 Forbidden` if permission denied

Example flow:
```
Plugin → Host API /kv/set
         ↓
         Extract plugin_id
         ↓
         Check KvWrite permission
         ↓
      Granted? → Execute operation
         ↓
      Denied? → Return 403
```

### 1.4 Permission Grant Storage

Permissions are stored in the `yh_plg_permission_grants` table:

```sql
CREATE TABLE yh_plg_permission_grants (
    id TEXT PRIMARY KEY,
    plugin_id TEXT NOT NULL,
    permission_key TEXT NOT NULL,
    granted BOOLEAN NOT NULL,
    created_at TIMESTAMP,
    updated_at TIMESTAMP
);
```

## 2. User Context Security

### 2.1 The Problem

Plugins need to know which user is making a request to implement user-scoped data access. However, allowing plugins to specify user identity via HTTP headers would enable identity forgery.

### 2.2 HMAC-Based Solution

We use **HMAC-SHA256** signatures to securely transmit user context from the host to plugins.

#### 2.2.1 Signature Generation (Public Gateway)

When the public gateway proxies user requests to plugins:

1. Extract user info from JWT: `user_id`, `username`, `role_id`
2. Compute signature: `HMAC-SHA256(user_id:username:role_id, secret)`
3. Add headers to proxied request:
   - `X-Plugin-User-ID`: user ID
   - `X-Plugin-User-Name`: username
   - `X-Plugin-User-Role`: role ID
   - `X-Plugin-User-Signature`: HMAC signature

#### 2.2.2 Signature Verification (Host API)

When Host API receives requests with user context headers:

1. Extract all 4 headers
2. Recompute expected signature using the same formula
3. Use **constant-time comparison** to verify signature
4. If valid: use the user context
5. If invalid: log warning and treat as anonymous (no user context)

#### 2.2.3 Security Properties

- **Forgery-proof**: Plugins cannot forge user identity without the HMAC secret
- **Timing-attack resistant**: Constant-time comparison prevents timing side-channels
- **Graceful degradation**: Invalid signatures are logged but don't crash requests
- **Fresh per-boot**: Secret is regenerated on each host start

### 2.3 Secret Management

The HMAC secret is:
- 32 bytes of cryptographically random data
- Generated once at startup in `PluginRuntimeManagerV2::new()`
- Stored in-memory only (never written to disk)
- Shared between public gateway and Host API through the manager singleton

## 3. Data Isolation

### 3.1 Key-Value Store Isolation

**All KV keys are automatically namespaced** with plugin ID:

```
User key: "my_setting"
Actual key: "plugin:com.example.myplugin:my_setting"
```

Plugins cannot access keys from other plugins.

### 3.2 SQLite Database Isolation

Each plugin gets its own SQLite database file:

```
{temp_dir}/extension/sqlite/{plugin_id}/
```

The physical path is never exposed to plugins. Plugins only receive a database name through the Host API and cannot access other plugins' databases.

### 3.3 Shared Records Isolation

The `yh_plg_shared_records` table isolates data by:
- `plugin_id` - only accessible by the owning plugin
- `collection` - logical grouping within the plugin
- `record_key` - unique identifier within the collection
- `owner_user_id` - user who owns this record

Plugins cannot query or modify records from other plugins.

### 3.4 VFS Isolation (Planned)

**Future enhancement**: Automatically prefix VFS paths with `/plugins/{plugin_id}/` to enforce filesystem isolation.

## 4. Lifecycle State Management

### 4.1 State Machine

Plugin lifecycle follows a strict state machine:

```
pending → installed → running → stopped (back to installed)
   ↓                      ↓
uninstalled         uninstalled
```

### 4.2 State Consistency Guarantees

**Problem**: If runtime startup/stop fails, database and memory state can diverge.

**Solution**: Transactional state updates with rollback:

#### Start Plugin
```
1. Check prerequisites (status = installed, not already running)
2. Record old state
3. Update DB: status → running
4. Try to start runtime
   Success: Keep running state, set handle
   Failure: Rollback DB to old state, log audit
```

#### Stop Plugin
```
1. Check current state (status = running)
2. Get runtime handle (don't remove yet)
3. Try to stop runtime
   Success: Remove handle, update DB: status → installed
   Failure: Keep handle and running state, log audit
```

All state changes are logged to `yh_plg_audit_logs`.

## 5. Best Practices for Plugin Developers

### 5.1 Declare Minimum Permissions

Only request permissions you actually need:

```json
// ❌ Bad: requesting unnecessary permissions
"permissions": ["auth-read", "vfs-read", "vfs-write", "db-sqlite", "network"]

// ✅ Good: minimal necessary permissions
"permissions": ["auth-read", "kv-read", "kv-write"]
```

### 5.2 Use Environment Variables for Configuration

**Never hardcode paths or credentials**:

```rust
// ❌ Bad
let config_path = "/some/path/config.toml";

// ✅ Good
let config_path = std::env::var("FILEUNI_PLUGIN_CONFIG_FILE")
    .expect("FILEUNI_PLUGIN_CONFIG_FILE must be set by host");
```

### 5.3 Respect User Context

When the host provides user context headers:

```rust
// Extract from headers
let user_id = headers.get("X-Plugin-User-ID");
let signature = headers.get("X-Plugin-User-Signature");

// If no user context, treat as anonymous
// If user context exists, use it for access control
if let Some(uid) = user_id {
    // User-scoped operation
    query_user_data(uid)
} else {
    // Anonymous operation
    return_public_data()
}
```

### 5.4 Never Trust Plugin Input

Always validate data from other plugins or users:

```rust
// ❌ Bad
let path = user_input;
write_file(&path, data);

// ✅ Good
let path = sanitize_path(user_input)?;
if !is_safe_path(&path) {
    return Err("invalid path");
}
write_file(&path, data);
```

### 5.5 Handle Permission Denials Gracefully

Host API calls may return 403. Handle them gracefully:

```rust
match host_api.kv_set("key", "value").await {
    Ok(_) => info!("Saved"),
    Err(e) if e.status() == 403 => {
        warn!("Missing kv-write permission, using in-memory cache");
        cache.insert("key", "value");
    }
    Err(e) => error!("Failed: {}", e),
}
```

## 6. Security Audit Checklist

For plugin reviewers and administrators:

- [ ] Plugin declares only necessary permissions
- [ ] No hardcoded credentials or secrets in plugin.json
- [ ] Plugin uses environment variables for configuration
- [ ] Network requests are justified and documented
- [ ] Database operations use plugin-specific tables only
- [ ] File operations use VFS Host API, not direct filesystem access
- [ ] User input is validated and sanitized
- [ ] Error messages don't leak sensitive information
- [ ] Plugin has no backdoor or phone-home code

## 7. Incident Response

If a security issue is discovered in a plugin:

1. **Revoke permissions**: Update `yh_plg_permission_grants` to deny access
2. **Stop plugin**: Use the admin API to stop the plugin immediately
3. **Audit logs**: Check `yh_plg_audit_logs` for suspicious activity
4. **Uninstall**: Remove the plugin entirely if compromised
5. **Notify users**: Inform affected users of the security issue

## 8. Future Enhancements

Planned security improvements:

1. **VFS path auto-prefixing**: Force all VFS paths under `/plugins/{plugin_id}/`
2. **SQL sandboxing**: Parse and validate SQL to restrict table access
3. **Resource quotas**: Limit CPU, memory, disk, network per plugin
4. **Plugin signing**: Verify cryptographic signatures on plugin packages
5. **Capability-based model**: Fine-grained capabilities beyond 17 permissions
6. **Audit log encryption**: Encrypt sensitive audit log entries

## References

- Main documentation: `docs/插件系统v2.md`
- File naming standards: `FILE_NAMING.md`
- Example plugins: `fileuni-extensions/fileuni-chat`, `fileuni-extensions/fileuni-email-manager`
