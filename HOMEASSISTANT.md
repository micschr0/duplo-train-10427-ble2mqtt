# Home Assistant Integration

## MQTT Sensors

Add to `configuration.yaml`:

```yaml
mqtt:
  sensor:
    # Status Sensor
    - name: "Duplo Train Status"
      unique_id: duplo_train_status
      state_topic: "duplo/train/state"
      value_template: "{{ value_json.status }}"
      json_attributes_topic: "duplo/train/state"
      icon: mdi:train
      availability:
        - topic: "duplo/train/availability"
          payload_available: "online"
          payload_not_available: "offline"

    # Battery Sensor
    - name: "Duplo Train Battery"
      unique_id: duplo_train_battery
      state_topic: "duplo/train/state"
      value_template: "{{ value_json.battery }}"
      device_class: battery
      unit_of_measurement: "%"
      entity_category: diagnostic
      availability:
        - topic: "duplo/train/availability"
          payload_available: "online"
          payload_not_available: "offline"

    # Attempts Sensor
    - name: "Duplo Train Attempts"
      unique_id: duplo_train_attempts
      state_topic: "duplo/train/state"
      value_template: "{{ value_json.attempts }}"
      icon: mdi:counter
      entity_category: diagnostic
      availability:
        - topic: "duplo/train/availability"
          payload_available: "online"
          payload_not_available: "offline"

    # Motor Sensor (commanded speed)
    - name: "Duplo Train Motor"
      unique_id: duplo_train_motor
      state_topic: "duplo/train/state"
      value_template: "{{ value_json.motor }}"
      icon: mdi:engine
      unit_of_measurement: "%"
      availability:
        - topic: "duplo/train/availability"
          payload_available: "online"
          payload_not_available: "offline"

    # Speed Sensor (measured from speedometer)
    - name: "Duplo Train Speed"
      unique_id: duplo_train_speed
      state_topic: "duplo/train/state"
      value_template: "{{ value_json.speed }}"
      icon: mdi:speedometer
      entity_category: diagnostic
      availability:
        - topic: "duplo/train/availability"
          payload_available: "online"
          payload_not_available: "offline"

    # Last Command Sensor
    - name: "Duplo Train Last Command"
      unique_id: duplo_train_last_command
      state_topic: "duplo/train/executed"
      value_template: "{{ value_json.cmd }}"
      icon: mdi:play-circle
      entity_category: diagnostic
      availability:
        - topic: "duplo/train/availability"
          payload_available: "online"
          payload_not_available: "offline"

  select:
    # LED Color
    - name: "Duplo Train LED"
      unique_id: duplo_train_led
      command_topic: "duplo/train/led/set"
      state_topic: "duplo/train/state"
      value_template: "{{ value_json.led }}"
      options:
        - "off"
        - white
        - green
        - yellow
        - light_blue
        - dark_blue
        - purple
        - purple_pink
        - light_pink
        - red_pink
        - red
      icon: mdi:led-on
      availability:
        - topic: "duplo/train/availability"
          payload_available: "online"
          payload_not_available: "offline"

    # Sound Action
    - name: "Duplo Train Sound"
      unique_id: duplo_train_sound
      command_topic: "duplo/train/sound/set"
      state_topic: "duplo/train/state"
      value_template: "{{ value_json.last_sound }}"
      options:
        - brake
        - station_departure
        - water_refill
        - horn
        - steam
      icon: mdi:music
      availability:
        - topic: "duplo/train/availability"
          payload_available: "online"
          payload_not_available: "offline"
```

## Template Sensor (Dynamic Status Icon)

Optional: Status icon changes based on connection state.

```yaml
template:
  - sensor:
      - name: "Duplo Train Connection"
        unique_id: duplo_train_connection
        state: "{{ states('sensor.duplo_train_status') }}"
        icon: >
          {% set s = states('sensor.duplo_train_status') %}
          {% if s == 'connected' %}mdi:bluetooth-connect
          {% elif s == 'connecting' %}mdi:bluetooth-search
          {% else %}mdi:bluetooth-off{% endif %}
```

## Automations

### Button Controller

Receives ZHA button events, sends MQTT commands.

```yaml
alias: DUPLO Train - Controller
description: Button → MQTT example
triggers:
  - trigger: event
    event_type: zha_event
    event_data:
      device_ieee: 5c:02:22:ff:fe:95:8c:25 # ← Your device IEEE
conditions:
  - condition: template
    value_template: "{{ trigger.event.data.command in ['on', 'off', 'step'] }}"
actions:
  - variables:
      button: >
        {% set c = trigger.event.data.command %} {% set s =
        trigger.event.data.params.step_mode | default(-1) %} {% if c == 'off'
        %}green {% elif c == 'on' %}blue {% elif c == 'step' and s == 0 %}yellow
        {% elif c == 'step' and s == 1 %}red {% else %}unknown{% endif %}
      commands:
        green: forward
        blue: boost
        yellow: backward
        red: stop
        unknown: stop
  - action: mqtt.publish
    data:
      topic: duplo/train/cmd
      payload: "{{ commands[button] }}"
mode: queued
max: 2

```

### Feedback Handler

Plays sounds on state changes, triggers light flash on executed commands.

```yaml
alias: DUPLO Train - Feedback Handler
description: State-Changes → Sounds, Executed → Flash
triggers:
  - trigger: state
    entity_id: sensor.duplo_train_attempts
    id: attempts_changed
  - trigger: state
    entity_id: sensor.duplo_train_status
    id: status_changed
  - trigger: mqtt
    topic: duplo/train/executed
    id: executed
actions:
  - variables:
      # ===== FEATURE FLAGS =====
      enable_sounds: true
      enable_flash: true
      # ==========================
  - choose:
      # Attempt 1 → Bell
      - conditions:
          - "{{ trigger.id == 'attempts_changed' }}"
          - "{{ trigger.to_state.state | int == 1 }}"
          - "{{ enable_sounds }}"
        sequence:
          - action: chime_tts.say
            data:
              chime_path: bells
              message: ""
              entity_id: media_player.wohnzimmer
              announce: true
              volume_level: 0.1

      # Attempt 2 → Voice Hint
      - conditions:
          - "{{ trigger.id == 'attempts_changed' }}"
          - "{{ trigger.to_state.state | int == 2 }}"
          - "{{ enable_sounds }}"
        sequence:
          - action: chime_tts.say
            data:
              chime_path: marimba
              cache: true
              message:
                - tts: Schalte den Zug ein.
                  tts_speed: 75
                - tts: Wenn er schon an ist, dann schalte ihn bitte einmal aus und dann wieder an.
                  offset: 1500
                  tts_speed: 90
              entity_id: media_player.wohnzimmer
              announce: true
              volume_level: 0.1

      # Attempt 3 → Error
      - conditions:
          - "{{ trigger.id == 'attempts_changed' }}"
          - "{{ trigger.to_state.state | int == 3 }}"
          - "{{ enable_sounds }}"
        sequence:
          - action: chime_tts.say
            data:
              chime_path: error
              cache: true
              message: Fehler, Frage Mama oder Papa.
              entity_id: media_player.wohnzimmer
              announce: true
              volume_level: 0.1
              tts_speed: 75

      # Start Scanning → Toast
      - conditions:
          - "{{ trigger.id == 'status_changed' }}"
          - "{{ trigger.from_state.state == 'standby' }}"
          - "{{ trigger.to_state.state == 'connecting' }}"
          - "{{ enable_sounds }}"
        sequence:
          - action: chime_tts.say
            data:
              chime_path: toast
              message: ""
              entity_id: media_player.wohnzimmer
              volume_level: 0.1

      - conditions:
          - "{{ trigger.id == 'executed' }}"
          - "{{ enable_flash }}"
        sequence:
          - variables:
              cmd: "{{ (trigger.payload | from_json).cmd }}"
              colors: # button colors on the remote
                forward: [0, 255, 0]
                boost: [0, 0, 255]
                backward: [255, 255, 0]
                stop: [255, 0, 0]
          - action: script.turn_on
            target:
              entity_id: script.duplo_train_flash_feedback
            data:
              variables:
                color: "{{ colors[cmd] | default([255,255,255]) }}"
mode: queued
max: 5
```

### Flash Feedback Script

```yaml
alias: DUPLO Train - Flash Feedback
description: Visual feedback by colored light flash
mode: restart
sequence: 
  - action: light.turn_on
    target:
      entity_id:
        - light.xyz
        - light.xyz2
        - light.xyz3
    data:
      rgb_color: "{{ color }}"
      brightness_pct: 100
      transition: 0
  - delay:
      milliseconds: 800
  - action: switch.turn_on
    target:
      entity_id:
        - switch.adaptive_lighting_wohnzimmer
        - switch.adaptive_lighting_wohnzimmer_stehlampe
```

## Dashboard Card

```yaml
type: entities
title: DUPLO Zug
entities:
  - entity: sensor.duplo_train_connection
    name: Status
  - entity: sensor.duplo_train_battery
    name: Batterie
  - entity: sensor.duplo_train_motor
    name: Motor
  - entity: sensor.duplo_train_last_command
    name: Letzter Befehl
```

## Entities Overview

| Entity | Type | Icon | Category |
|--------|------|------|----------|
| `sensor.duplo_train_status` | Status | mdi:train | - |
| `sensor.duplo_train_connection` | Template | dynamic | - |
| `sensor.duplo_train_battery` | Battery | auto | diagnostic |
| `sensor.duplo_train_attempts` | Counter | mdi:counter | diagnostic |
| `sensor.duplo_train_motor` | Motor % | mdi:engine | - |
| `sensor.duplo_train_speed` | Speed | mdi:speedometer | diagnostic |
| `sensor.duplo_train_last_command` | Command | mdi:play-circle | diagnostic |
| `select.duplo_train_led` | LED colour (11 options) | mdi:led-on | - |
| `select.duplo_train_sound` | Sound action (5 options) | mdi:music | - |
