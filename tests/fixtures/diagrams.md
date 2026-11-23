# Diagrams

## Mermaid flowchart

```mermaid
flowchart TD
    A[Request] --> B{Cached?}
    B -->|yes| C[Serve cache]
    B -->|no| D[Fetch origin]
    D --> E[(Store)]
    E --> C
```

## Mermaid left to right

```mermaid
graph LR
    Build --> Test --> Deploy
    Test -.->|flaky| Build
```

## Sequence

```mermaid
sequenceDiagram
    participant U as User
    participant S as Server
    U->>S: GET /page
    alt cached
        S-->>U: 200 (cache)
    else miss
        S->>S: render
        S-->>U: 200
    end
    Note right of S: logs request
```

## State

```mermaid
stateDiagram-v2
    [*] --> Draft
    Draft --> Review : submit
    Review --> Draft : changes
    Review --> Done : approve
    Done --> [*]
```

## Graphviz

```dot
digraph deps {
    app -> core;
    app -> ui;
    ui -> core;
    core -> util [style=dashed];
}
```

## Charts

```chart
title: Requests per day
Mon: 120
Tue: 340
Wed: 290
Thu: 410
Fri: 180
```

```chart
type: line
title: Latency (ms)
series: p50, p95
08:00: 20, 80
10:00: 25, 120
12:00: 40, 200
14:00: 30, 150
16:00: 22, 90
```

```mermaid
pie title Browser share
    "Firefox" : 42
    "Chrome" : 48
    "Other" : 10
```

```mermaid
xychart-beta
    title "Monthly sign-ups"
    x-axis [Jan, Feb, Mar, Apr]
    bar [30, 45, 60, 52]
```

## Unsupported

```mermaid
gantt
    title Not drawn
```
