To to dump Meshtastic's traffic.

- Messaging features:
  - Decode direct messages if private and public node's keys are stored in config
  - Decode channel messages if key is stored in config

- Connection features:
  - MQTT
  - Serial
  - TCP


Application uses two config files:

- `connection.yaml`
- `keys.yaml`


# `connection.yaml` describes connection settings: TCP, Serial, MQTT or Multicast
## TCP:
```yaml
mode: !TCP
  connect_to: 192.168.1.184:4403
  heartbeat_seconds: 10
```

## MQTT:
```yaml
mode: !MQTT
  server_addr: mqtt-server.com
  server_port: 1883
  username: cat
  password: to big cat
  subscribe:
  - msh/2/e/+/+
  - msh/RU/2/+/+
```

## Serial:
```yaml
mode: !Serial
  tty: COM6
  baudrate: 115200
  heartbeat_seconds: 10
```

## Multicast:
```yaml
mode: !Multicast
  listen_address: 224.0.0.69:4403
```

# `keys.yaml` describes number of peers and their keys

```yaml
channels:
- name: LongFast
  key: 1PG7OiApB1nwvP+rz05pAQ==
- name: ShortFast
  key: 1PG7OiApB1nwvP+rz05pAQ==
peers:
- name: OwnedPeer
  node_id: '!aabbccdd'
  highlight: false
  private_key: mKsioP5e59jZiW9yYjzAPDnfvsIk1+p+g80ke09wkls=
- name: RemotePeer
  node_id: '!ddccbbaa'
  highlight: true
  public_key: +AszX0jkaklCkfjdqrJ6N/L9PDZYvPIhDLj8iiAEjxU=
```
