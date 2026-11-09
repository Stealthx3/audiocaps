// SPDX-License-Identifier: LGPL-2.1-or-later
//! Probe scope: what the application asks for. Information on-demand is built into the API shape:
//! every probe takes a [`ProbeScope`], and nothing outside it is read, cached or reported.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::endpoint::{Direction, EndpointClass, EndpointId};
use crate::path::AccessPath;

/// A selection of items: everything, or only the listed ones. An empty `Only` selects nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "select", content = "items", rename_all = "snake_case")]
pub enum Selection<T> {
    /// Every item.
    All,
    /// Only these items.
    Only(Vec<T>),
}

impl<T: PartialEq> Selection<T> {
    /// Whether `item` is selected.
    pub fn includes(&self, item: &T) -> bool {
        match self {
            Self::All => true,
            Self::Only(items) => items.contains(item),
        }
    }

    /// Whether nothing is selected.
    pub fn is_empty(&self) -> bool {
        matches!(self, Self::Only(items) if items.is_empty())
    }
}

/// The seven layers AudioCaps reads, from the far end of the cable to the app's sandbox.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum LayerKind {
    /// The connected sink and what sits behind it (ELD, EDID, CEC).
    SinkChain,
    /// Host hardware: PCI/USB IDs, codecs, pins, USB alt-settings, DMI.
    HostHardware,
    /// Kernel driver, module parameters, jack state, runtime PM.
    KernelDriver,
    /// ALSA userspace: hw_params ranges, controls, UCM, plugin chain, MIDI ports.
    AlsaUserspace,
    /// Sound server: PipeWire, WirePlumber, PulseAudio, JACK and others.
    SoundServer,
    /// Wireless: Bluetooth codecs, profiles, LE Audio.
    Wireless,
    /// Environment: sandbox, permissions, realtime, distro, library versions.
    Environment,
}

impl LayerKind {
    /// Every layer, in order from sink to app.
    pub const ALL: [LayerKind; 7] = [
        Self::SinkChain,
        Self::HostHardware,
        Self::KernelDriver,
        Self::AlsaUserspace,
        Self::SoundServer,
        Self::Wireless,
        Self::Environment,
    ];
}

/// Groups of fields an application can ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FieldGroup {
    /// Endpoint identity, names and correlation keys.
    Identity,
    /// Sample formats and bit depths.
    Formats,
    /// Sample rates.
    Rates,
    /// Channel counts.
    Channels,
    /// Channel maps.
    ChannelMaps,
    /// Compressed passthrough codecs.
    Passthrough,
    /// Bluetooth codecs.
    BluetoothCodecs,
    /// Latency ranges and estimates.
    Latency,
    /// Whether direct (exclusive) access is available.
    DirectAccess,
    /// Sink record (identity, SADs, speaker allocation).
    Sink,
    /// Effective sets per access path.
    Effective,
    /// Path reports with hops.
    Paths,
    /// Diagnostic findings.
    Findings,
}

/// What a probe may read and report. Build it with [`ProbeScope::nothing`] and the `with_*`
/// methods, or start from [`ProbeScope::everything`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProbeScope {
    /// Endpoint classes to consider.
    pub endpoint_classes: Selection<EndpointClass>,
    /// Directions to consider.
    pub directions: Selection<Direction>,
    /// Specific endpoints (by id) to consider.
    pub endpoints: Selection<EndpointId>,
    /// Layers to read.
    pub layers: Selection<LayerKind>,
    /// Field groups to report.
    pub fields: Selection<FieldGroup>,
    /// Access paths to compute effective sets and path reports for.
    pub access_paths: Selection<AccessPath>,
}

impl ProbeScope {
    /// Selects nothing. Add what the application needs with the `with_*` methods.
    pub fn nothing() -> Self {
        Self {
            endpoint_classes: Selection::Only(vec![]),
            directions: Selection::Only(vec![]),
            endpoints: Selection::All,
            layers: Selection::Only(vec![]),
            fields: Selection::Only(vec![]),
            access_paths: Selection::Only(vec![]),
        }
    }

    /// Selects everything the library can read (passive probes only; opt-ins stay separate calls).
    pub fn everything() -> Self {
        Self {
            endpoint_classes: Selection::All,
            directions: Selection::All,
            endpoints: Selection::All,
            layers: Selection::All,
            fields: Selection::All,
            access_paths: Selection::All,
        }
    }

    /// Adds an endpoint class.
    pub fn with_class(mut self, class: EndpointClass) -> Self {
        push(&mut self.endpoint_classes, class);
        self
    }

    /// Adds a direction.
    pub fn with_direction(mut self, direction: Direction) -> Self {
        push(&mut self.directions, direction);
        self
    }

    /// Restricts to one more specific endpoint.
    pub fn with_endpoint(mut self, id: EndpointId) -> Self {
        match &mut self.endpoints {
            Selection::All => self.endpoints = Selection::Only(vec![id]),
            Selection::Only(v) => v.push(id),
        }
        self
    }

    /// Adds a layer.
    pub fn with_layer(mut self, layer: LayerKind) -> Self {
        push(&mut self.layers, layer);
        self
    }

    /// Adds a field group.
    pub fn with_field(mut self, field: FieldGroup) -> Self {
        push(&mut self.fields, field);
        self
    }

    /// Adds an access path.
    pub fn with_access_path(mut self, path: AccessPath) -> Self {
        push(&mut self.access_paths, path);
        self
    }

    /// Whether an endpoint of this class, direction and id is in scope.
    pub fn includes_endpoint(&self, class: EndpointClass, dir: Direction, id: &EndpointId) -> bool {
        self.endpoint_classes.includes(&class)
            && self.directions.includes(&dir)
            && self.endpoints.includes(id)
    }
}

fn push<T: PartialEq>(sel: &mut Selection<T>, item: T) {
    if let Selection::Only(items) = sel
        && !items.contains(&item)
    {
        items.push(item);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_selects_nothing() {
        let s = ProbeScope::nothing();
        assert!(!s.layers.includes(&LayerKind::SinkChain));
        assert!(!s.includes_endpoint(
            EndpointClass::Hardware,
            Direction::Playback,
            &EndpointId("alsa:card0/pcm0p".into())
        ));
    }

    #[test]
    fn builder_adds_only_what_was_asked() {
        let s = ProbeScope::nothing()
            .with_class(EndpointClass::Hardware)
            .with_direction(Direction::Playback)
            .with_layer(LayerKind::SinkChain)
            .with_field(FieldGroup::Passthrough);
        assert!(s.layers.includes(&LayerKind::SinkChain));
        assert!(!s.layers.includes(&LayerKind::SoundServer));
        assert!(s.fields.includes(&FieldGroup::Passthrough));
        assert!(!s.fields.includes(&FieldGroup::Rates));
        assert!(!s.includes_endpoint(
            EndpointClass::Bluetooth,
            Direction::Playback,
            &EndpointId("bt:x".into())
        ));
    }

    #[test]
    fn endpoint_restriction() {
        let id = EndpointId("alsa:card1/pcm3p".into());
        let s = ProbeScope::everything().with_endpoint(id.clone());
        assert!(s.includes_endpoint(EndpointClass::Hardware, Direction::Playback, &id));
        assert!(!s.includes_endpoint(
            EndpointClass::Hardware,
            Direction::Playback,
            &EndpointId("alsa:card0/pcm0p".into())
        ));
    }
}
