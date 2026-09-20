// Generated with: branchy snapshot
// A stand-in so the interface can be opened in a browser without the app.
window.BRANCHY_FIXTURE = {
  "areas": [
    {
      "id": "a0",
      "name": "Hard skills",
      "color": "#4fd1c5",
      "done": 6,
      "total": 10
    },
    {
      "id": "a1",
      "name": "Career",
      "color": "#eab64d",
      "done": 0,
      "total": 3
    },
    {
      "id": "a2",
      "name": "Health",
      "color": "#8fd17c",
      "done": 2,
      "total": 5
    },
    {
      "id": "a3",
      "name": "Social",
      "color": "#ef8093",
      "done": 1,
      "total": 3
    }
  ],
  "nodes": [
    {
      "id": "n0",
      "name": "School algebra",
      "note": "",
      "area": "a0",
      "priority": 3,
      "done": true,
      "prereqs": [],
      "dependents": [
        "n1",
        "n2"
      ],
      "status": "done",
      "tier": 0
    },
    {
      "id": "n1",
      "name": "Linear algebra",
      "note": "",
      "area": "a0",
      "priority": 5,
      "done": true,
      "prereqs": [
        "n0"
      ],
      "dependents": [
        "n3"
      ],
      "status": "done",
      "tier": 1
    },
    {
      "id": "n2",
      "name": "Calculus",
      "note": "",
      "area": "a0",
      "priority": 6,
      "done": true,
      "prereqs": [
        "n0"
      ],
      "dependents": [
        "n3"
      ],
      "status": "done",
      "tier": 1
    },
    {
      "id": "n3",
      "name": "Probability theory",
      "note": "",
      "area": "a0",
      "priority": 7,
      "done": false,
      "prereqs": [
        "n1",
        "n2"
      ],
      "dependents": [
        "n11"
      ],
      "status": "available",
      "tier": 2
    },
    {
      "id": "n4",
      "name": "C++11/14 core",
      "note": "",
      "area": "a0",
      "priority": 2,
      "done": true,
      "prereqs": [],
      "dependents": [
        "n5",
        "n10"
      ],
      "status": "done",
      "tier": 0
    },
    {
      "id": "n5",
      "name": "Threads and atomics",
      "note": "",
      "area": "a0",
      "priority": 5,
      "done": true,
      "prereqs": [
        "n4"
      ],
      "dependents": [
        "n6",
        "n7"
      ],
      "status": "done",
      "tier": 1
    },
    {
      "id": "n6",
      "name": "Lock-free structures",
      "note": "SPSC ring buffer, MPMC queue, the ABA problem.",
      "area": "a0",
      "priority": 9,
      "done": false,
      "prereqs": [
        "n5"
      ],
      "dependents": [
        "n11"
      ],
      "status": "available",
      "tier": 2
    },
    {
      "id": "n7",
      "name": "Benchmarking",
      "note": "",
      "area": "a0",
      "priority": 6,
      "done": false,
      "prereqs": [
        "n5"
      ],
      "dependents": [
        "n11"
      ],
      "status": "available",
      "tier": 2
    },
    {
      "id": "n8",
      "name": "Rust ownership",
      "note": "",
      "area": "a0",
      "priority": 2,
      "done": true,
      "prereqs": [],
      "dependents": [
        "n9"
      ],
      "status": "done",
      "tier": 0
    },
    {
      "id": "n9",
      "name": "Index-based graphs",
      "note": "Ids instead of pointers. The arena pattern.",
      "area": "a0",
      "priority": 5,
      "done": false,
      "prereqs": [
        "n8"
      ],
      "dependents": [],
      "status": "available",
      "tier": 1
    },
    {
      "id": "n10",
      "name": "LeetCode refresh",
      "note": "",
      "area": "a1",
      "priority": 6,
      "done": false,
      "prereqs": [
        "n4"
      ],
      "dependents": [
        "n18"
      ],
      "status": "available",
      "tier": 1
    },
    {
      "id": "n11",
      "name": "Limit order book",
      "note": "A price-time priority matching engine, measured rather than guessed at.",
      "area": "a1",
      "priority": 10,
      "done": false,
      "prereqs": [
        "n3",
        "n6",
        "n7"
      ],
      "dependents": [
        "n20"
      ],
      "status": "locked",
      "tier": 3
    },
    {
      "id": "n12",
      "name": "Gym membership",
      "note": "",
      "area": "a2",
      "priority": 1,
      "done": true,
      "prereqs": [],
      "dependents": [
        "n15"
      ],
      "status": "done",
      "tier": 0
    },
    {
      "id": "n13",
      "name": "Sleep schedule",
      "note": "",
      "area": "a2",
      "priority": 8,
      "done": true,
      "prereqs": [],
      "dependents": [
        "n16"
      ],
      "status": "done",
      "tier": 0
    },
    {
      "id": "n14",
      "name": "Weekly meal plan",
      "note": "",
      "area": "a2",
      "priority": 7,
      "done": false,
      "prereqs": [],
      "dependents": [
        "n15"
      ],
      "status": "available",
      "tier": 0
    },
    {
      "id": "n15",
      "name": "Strength base",
      "note": "",
      "area": "a2",
      "priority": 5,
      "done": false,
      "prereqs": [
        "n12",
        "n14"
      ],
      "dependents": [
        "n16"
      ],
      "status": "locked",
      "tier": 1
    },
    {
      "id": "n16",
      "name": "Endurance",
      "note": "",
      "area": "a2",
      "priority": 3,
      "done": false,
      "prereqs": [
        "n13",
        "n15"
      ],
      "dependents": [],
      "status": "locked",
      "tier": 2
    },
    {
      "id": "n17",
      "name": "English B2 to C1",
      "note": "",
      "area": "a3",
      "priority": 6,
      "done": true,
      "prereqs": [],
      "dependents": [
        "n18",
        "n19"
      ],
      "status": "done",
      "tier": 0
    },
    {
      "id": "n18",
      "name": "Mock interviews",
      "note": "",
      "area": "a3",
      "priority": 8,
      "done": false,
      "prereqs": [
        "n10",
        "n17"
      ],
      "dependents": [
        "n20"
      ],
      "status": "locked",
      "tier": 2
    },
    {
      "id": "n19",
      "name": "Reach out to traders",
      "note": "",
      "area": "a3",
      "priority": 4,
      "done": false,
      "prereqs": [
        "n17"
      ],
      "dependents": [],
      "status": "available",
      "tier": 1
    },
    {
      "id": "n20",
      "name": "HFT interviews",
      "note": "",
      "area": "a1",
      "priority": 10,
      "done": false,
      "prereqs": [
        "n11",
        "n18"
      ],
      "dependents": [],
      "status": "locked",
      "tier": 4
    }
  ],
  "queue": [
    "n6",
    "n3",
    "n14",
    "n7",
    "n10",
    "n9",
    "n19"
  ],
  "cycles": [],
  "tally": {
    "total": 21,
    "done": 9,
    "available": 7
  }
}
;
