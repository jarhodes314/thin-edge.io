---
title: Sending Measurements
tags: [Getting Started, Telemetry]
sidebar_position: 5
description: How to send measurements with %%te%%
---

# Sending Measurements

Once your %%te%% device is configured and connected to an IoT cloud provider, you can start sending measurements.
Refer to [Connecting to Cumulocity](./connect-c8y.md) or tutorials for other cloud providers 
to learn how to connect your %%te%% device to an IoT cloud provider. 

In this tutorial, we'll see how different kinds of measurements are represented in %%te%% JSON format and 
how they can be sent to the connected cloud provider.
For a more detailed specification of this data format, refer to [%%te%% JSON Specification](../understand/thin-edge-json.md).

## Sending measurements

A simple single-valued measurement like a temperature measurement, can be represented in %%te%% JSON as follows:

```json
{"temperature": 25}
```

with the key-value pair representing the measurement type and the numeric value of the measurement.

This measurement can be sent from the %%te%% device to the cloud by publishing this message to the `te/+/+/+/+/m/+` MQTT topic.
Processes running on the %%te%% device can publish messages to the local MQTT broker using any MQTT client or library.
In this tutorial, we'll be using the `tedge mqtt pub` command line utility for demonstration purposes.

The temperature measurement described above can be sent using the `tedge mqtt pub` command as follows:

```sh te2mqtt formats=v1
tedge mqtt pub te/device/main///m/environment '{"temperature": 25}'
```

The first argument to the `tedge mqtt pub` command is the topic to which the measurements must be published to.
The second argument is the %%te%% JSON representation of the measurement itself.

When connected to a cloud provider, a message mapper component for that cloud provider would be running as a daemon, 
listening to any measurements published to `te/+/+/+/+/m/+`.
The mapper, on receipt of these %%te%% JSON measurements, will map those measurements to their equivalent
cloud provider native representation and send it to that cloud.

For example, when the device is connected to Cumulocity, the Cumulocity mapper component will be performing these actions.
To check if these measurements have reached Cumulocity, login to your Cumulocity dashboard and navigate to:

**Device Management** &rarr; **Devices** &rarr; **All devices** &rarr; `device-id` &rarr; **Measurements**

You can see if your temperature measurement is appearing in the dashboard.

## Complex measurements

You can represent measurements that are far more complex than the single-valued ones described above using the %%te%% JSON format.

A multi-valued measurement like `three_phase_current` that consists of `L1`, `L2` and `L3` values,
representing the current on each phase can be represented as follows:

```json
{
  "three_phase_current": {
    "L1": 9.5,
    "L2": 10.3,
    "L3": 8.8
  }
}
```

Here is another complex message consisting of single-valued measurements: `temperature` and `pressure` 
along with a multi-valued `coordinate` measurement, all sharing a single timestamp captured as `time`.

```json
{
  "time": "2020-10-15T05:30:47+00:00",
  "temperature": 25,
  "current": {
    "L1": 9.5,
    "L2": 10.3,
    "L3": 8.8
  },
  "pressure": 98
}
```

The `time` field is not a regular measurement like `temperature` or `pressure` but a special reserved field.
Refer to [%%te%% JSON Specification](../understand/thin-edge-json.md) for more details on the kinds of telemetry 
data that can be represented in %%te%% JSON format and the reserved fields like `time` used in the above example.

## Sending measurements to child devices

If valid %%te%% JSON measurements are published to the `te/device/<child-id>///m/<measurement-type>` topic,
the measurements are recorded under a child device of your %%te%% device.

Given your desired child device ID is `child1`, publish a %%te%% JSON message to the following topic where the measurement type is set to `environment`:

```sh te2mqtt formats=v1
tedge mqtt pub te/device/child1///m/environment '{"temperature": 25}'
```

Then, you will see a child device with the name `child1` is created in your Cumulocity tenant,
and the measurement is recorded in `Measurements` of the `child1` device.

## Error detection

If the data published to the measurements topic are not valid %%te%% JSON measurements, those won't be
sent to the cloud but instead you'll get a feedback on the `te/errors` topic, if you subscribe to it.
The error messages published to this topic will be highly verbose and may change in the future.
So, use it only for debugging purposes during the development phase and it should **NOT** be used for any automation.

You can subscribe to the error topic as follows:

```sh te2mqtt formats=v1
tedge mqtt sub te/errors
```
