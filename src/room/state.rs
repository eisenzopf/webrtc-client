impl Room {
    pub fn new(id: String, media_settings: MediaSettings) -> Self {
        Self {
            id,
            peers: Vec::new(),
            media_settings,
            media_relays: HashMap::new(),
            recording_enabled: false,
            connected_pairs: HashSet::new(),
        }
    }

    pub fn add_peer(&mut self, peer_connection: PeerConnection) -> Result<()> {
        if self.peers.len() >= self.media_settings.max_participants {
            return Err(Error::Room("Room is full".to_string()));
        }
        self.peers.push(peer_connection);
        Ok(())
    }

    pub fn remove_peer(&mut self, peer_id: &str) {
        self.peers.retain(|(id, _)| id != peer_id);
        // Clean up connected pairs involving this peer
        self.connected_pairs.retain(|(p1, p2)| p1 != peer_id && p2 != peer_id);
    }
} 