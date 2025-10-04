# IP封禁功能说明

## 功能概述

该功能会自动跟踪每个IP的认证失败次数，当失败次数达到5次时，会永久封禁该IP地址。

## 实现细节

### 1. 失败次数统计

- 每次认证失败时，系统会记录该IP的失败次数
- 失败次数达到5次时，IP会被永久加入封禁列表
- 认证成功时，会重置该IP的失败计数

### 2. IP封禁检查

- 在认证中间件中，首先检查IP是否在封禁列表中
- 如果IP已被封禁，直接返回403 Forbidden状态码
- 封禁的IP无法再次尝试认证

### 3. 日志记录

系统会记录以下信息：

- 每次认证失败时记录IP和当前失败次数
- IP被封禁时记录警告日志
- 被封禁的IP尝试访问时记录警告日志

## API接口

### 查看被封禁的IP列表

```bash
GET /banned-ips
```

**响应示例：**

```json
{
  "status": "ok",
  "banned_ips": [
    "192.168.1.100",
    "10.0.0.50"
  ],
  "count": 2
}
```

### 解封指定IP

```bash
GET /unban/:ip
```

**示例：**

```bash
curl http://localhost:8080/unban/192.168.1.100
```

**响应示例：**

```json
{
  "status": "ok",
  "message": "IP 192.168.1.100 has been unbanned"
}
```

## 错误响应

### IP已被封禁

```json
{
  "error": {
    "message": "Your IP has been permanently banned due to multiple failed authentication attempts",
    "type": "ip_banned"
  }
}
```

**状态码：** 403 Forbidden

### 认证失败

```json
{
  "error": {
    "message": "Invalid authorization token",
    "type": "auth_error"
  }
}
```

**状态码：** 401 Unauthorized

## 配置说明

目前失败次数阈值硬编码为5次，位于 `src/state.rs` 文件中：

```rust
ip_ban_manager: Arc::new(IpBanManager::new(5)), // 失败5次封禁
```

如需修改阈值，可以修改这个数字并重新编译。

## 注意事项

1. **永久封禁**：当前实现的是永久封禁，IP被封禁后只能通过管理接口手动解封
2. **内存存储**：所有数据存储在内存中，服务重启后封禁记录会丢失
3. **代理服务器**：如果使用反向代理（如Nginx），请确保正确配置X-Forwarded-For头，否则可能获取到代理服务器的IP而非真实客户端IP

## 后续改进建议

1. 将封禁阈值配置化，添加到config.json中
2. 支持持久化存储，使用数据库或文件保存封禁记录
3. 添加临时封禁功能（例如封禁1小时后自动解封）
4. 支持IP白名单功能
5. 添加封禁时间戳和封禁原因记录
