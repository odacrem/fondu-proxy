# Fondu CMS Backend Design

A Content Management System designed to run on Fastly's Compute@Edge platform, serving as the "fondu backend" for fondu-proxy.

## Table of Contents

1. [Overview](#overview)
2. [Architecture](#architecture)
3. [Storage Strategy](#storage-strategy)
4. [Data Models](#data-models)
5. [API Design](#api-design)
6. [Content Authoring](#content-authoring)
7. [Caching Strategy](#caching-strategy)
8. [Personalization Engine](#personalization-engine)
9. [Multi-Tenancy](#multi-tenancy)
10. [Security](#security)
11. [Implementation Roadmap](#implementation-roadmap)

---

## Overview

### Purpose

The Fondu CMS Backend provides a managed content delivery system that:

- Serves JSON directives to fondu-proxy for HTML transformation
- Enables non-technical users to manage dynamic content components
- Supports personalization, A/B testing, and targeted content delivery
- Runs entirely at the edge for ultra-low latency

### Design Principles

1. **Edge-First**: All content serving happens at Fastly's edge POPs
2. **Fail-Open**: If CMS data is unavailable, fondu-proxy serves original content
3. **Streaming-Compatible**: Designed for fondu-proxy's streaming HTML rewriter
4. **Declarative**: Content authors specify "what" not "how"
5. **Composable**: Components are reusable building blocks

---

## Architecture

### High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           Content Authors                                │
│                    (Marketing, Editorial, Product)                       │
└────────────────────────────────┬────────────────────────────────────────┘
                                 │
                                 ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                        CMS Admin Interface                               │
│              (Web UI - hosted separately or as SPA)                      │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐    │
│  │ Components  │  │   Pages     │  │   Rules     │  │  Analytics  │    │
│  │   Editor    │  │   Manager   │  │   Engine    │  │  Dashboard  │    │
│  └─────────────┘  └─────────────┘  └─────────────┘  └─────────────┘    │
└────────────────────────────────┬────────────────────────────────────────┘
                                 │ Management API
                                 ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                     Fondu CMS Backend                                    │
│                   (Fastly Compute@Edge)                                  │
│                                                                          │
│  ┌──────────────────────────────────────────────────────────────────┐  │
│  │                      Request Router                               │  │
│  │   /api/pages/{path}  →  Page Resolution                          │  │
│  │   /api/manage/*      →  Management API                           │  │
│  │   /api/preview/*     →  Preview Mode                             │  │
│  └──────────────────────────────────────────────────────────────────┘  │
│                                 │                                        │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐                  │
│  │   Content    │  │    Rules     │  │  Component   │                  │
│  │   Resolver   │  │   Evaluator  │  │   Renderer   │                  │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘                  │
│         │                 │                 │                           │
│         └─────────────────┴─────────────────┘                           │
│                           │                                              │
│  ┌──────────────────────────────────────────────────────────────────┐  │
│  │                    Storage Layer                                  │  │
│  │   ┌────────────┐  ┌────────────┐  ┌────────────┐                 │  │
│  │   │  KV Store  │  │   Object   │  │  Config    │                 │  │
│  │   │ (metadata) │  │   Store    │  │   Store    │                 │  │
│  │   │            │  │  (content) │  │  (tenant)  │                 │  │
│  │   └────────────┘  └────────────┘  └────────────┘                 │  │
│  └──────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────┘
                                 │
                                 │ JSON Response
                                 ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                        Fondu Proxy                                       │
│                   (Fastly Compute@Edge)                                  │
│                                                                          │
│   Receives JSON directives, applies HTML transformations                 │
└─────────────────────────────────────────────────────────────────────────┘
```

### Component Interaction Flow

```
1. Client Request → Fondu Proxy
2. Fondu Proxy → CMS Backend: GET /api/pages/homepage
3. CMS Backend:
   a. Parse request path and headers
   b. Lookup page configuration in KV Store
   c. Evaluate targeting rules (geo, device, user segment)
   d. Resolve component references
   e. Fetch component HTML from Object Store
   f. Assemble JSON response
4. CMS Backend → Fondu Proxy: JSON with selectors/components
5. Fondu Proxy applies transformations
6. Client receives modified HTML
```

---

## Storage Strategy

Fastly Compute@Edge provides several storage options. This design uses a hybrid approach:

### Storage Components

| Store Type | Purpose | Characteristics |
|------------|---------|-----------------|
| **KV Store** | Page configurations, routing rules, component metadata | Fast key-value lookups, ~1KB values |
| **Object Store** | Component HTML content, large templates | Larger objects, streaming reads |
| **Config Store** | Tenant settings, feature flags, secrets | Read-only, deploy-time config |

### Key Schema Design

#### KV Store Keys

```
# Page configurations
page:{tenant}:{path}                    → PageConfig JSON
page:acme:/homepage                     → {...}
page:acme:/products/widgets             → {...}

# Component metadata
component:{tenant}:{component_id}       → ComponentMeta JSON
component:acme:hero-banner-v2           → {...}

# Targeting rules
rule:{tenant}:{rule_id}                 → Rule JSON
rule:acme:geo-us-promo                  → {...}

# Page-to-component mappings (for fast lookups)
mapping:{tenant}:{path}                 → ComponentList JSON
mapping:acme:/homepage                  → ["hero-banner", "promo-sidebar"]

# A/B test configurations
abtest:{tenant}:{test_id}               → ABTestConfig JSON
abtest:acme:homepage-hero-test          → {...}
```

#### Object Store Keys

```
# Component HTML content
html:{tenant}:{component_id}:{version}  → Raw HTML string
html:acme:hero-banner-v2:live           → "<div class='hero'>...</div>"
html:acme:hero-banner-v2:draft          → "<div class='hero'>...</div>"

# Templates (larger, reusable)
template:{tenant}:{template_id}         → Template HTML
template:acme:product-card              → "<article>{{title}}...</article>"
```

### Storage Limits & Considerations

| Store | Max Key Size | Max Value Size | Latency |
|-------|-------------|----------------|---------|
| KV Store | 1KB | 25MB | ~1-5ms |
| Object Store | 1KB | 5GB | ~5-20ms |
| Config Store | 256B | 8KB | ~0ms (cached) |

**Design Decision**: Keep frequently-accessed metadata in KV Store; store larger HTML blobs in Object Store. Use Config Store for tenant-level settings that change infrequently.

---

## Data Models

### Core Entities

#### Component

The fundamental unit of content - a reusable HTML snippet.

```rust
struct Component {
    id: String,              // Unique identifier: "hero-banner-v2"
    tenant_id: String,       // Tenant ownership
    name: String,            // Human-readable: "Homepage Hero Banner"
    description: Option<String>,

    // Content
    html: String,            // The actual HTML content
    css: Option<String>,     // Optional scoped CSS

    // Metadata
    selector_hint: String,   // Suggested CSS selector: "#hero"
    default_op: Operation,   // Default operation: Replace, Append, etc.

    // Versioning
    version: u32,
    status: ComponentStatus, // Draft, Published, Archived

    // Tracking
    created_at: DateTime,
    updated_at: DateTime,
    created_by: String,
}

enum ComponentStatus {
    Draft,
    Published,
    Scheduled,
    Archived,
}

enum Operation {
    Replace,
    Append,
    Prepend,
    Before,
    After,
}
```

#### Page Configuration

Defines what components appear on a specific page/route.

```rust
struct PageConfig {
    path: String,            // URL path: "/homepage" or "/products/*"
    tenant_id: String,

    // Component placements
    placements: Vec<Placement>,

    // Page-level settings
    cache_ttl: Option<u32>,  // Override default TTL
    enabled: bool,

    // Metadata
    created_at: DateTime,
    updated_at: DateTime,
}

struct Placement {
    selector: String,        // CSS selector: "#promo-banner"
    op: Operation,           // How to inject: Replace, Append, etc.

    // What to inject (one of):
    component_id: Option<String>,      // Direct component reference
    rule_id: Option<String>,           // Rule-based selection
    ab_test_id: Option<String>,        // A/B test

    // Fallback if rule/test fails
    fallback_component_id: Option<String>,

    priority: i32,           // Order of evaluation (higher = first)
}
```

#### Targeting Rule

Conditional logic for personalized content delivery.

```rust
struct Rule {
    id: String,
    tenant_id: String,
    name: String,

    // Conditions (all must match)
    conditions: Vec<Condition>,

    // Result when conditions match
    component_id: String,

    // Priority for rule ordering
    priority: i32,

    // Scheduling
    start_date: Option<DateTime>,
    end_date: Option<DateTime>,

    enabled: bool,
}

struct Condition {
    field: ConditionField,
    operator: Operator,
    value: String,
}

enum ConditionField {
    // Geographic
    Country,
    Region,
    City,

    // Device
    DeviceType,        // mobile, tablet, desktop
    Browser,
    OS,

    // Request
    Header(String),    // Custom header value
    Cookie(String),    // Cookie value
    QueryParam(String),

    // User (requires integration)
    UserSegment,
    IsAuthenticated,

    // Time
    DayOfWeek,
    Hour,

    // Random (for sampling)
    RandomPercent,
}

enum Operator {
    Equals,
    NotEquals,
    Contains,
    StartsWith,
    EndsWith,
    GreaterThan,
    LessThan,
    In,           // value is comma-separated list
    NotIn,
    Matches,      // regex
}
```

#### A/B Test

Split testing configuration.

```rust
struct ABTest {
    id: String,
    tenant_id: String,
    name: String,

    // Variants
    variants: Vec<Variant>,

    // Traffic allocation
    allocation_method: AllocationMethod,

    // Status
    status: TestStatus,
    start_date: Option<DateTime>,
    end_date: Option<DateTime>,

    // Analytics
    goal_type: GoalType,
    goal_selector: Option<String>,
}

struct Variant {
    id: String,
    name: String,           // "Control", "Variant A"
    component_id: String,
    weight: u32,            // Percentage: 50 = 50%
}

enum AllocationMethod {
    Random,                 // Pure random per request
    Sticky,                 // Consistent per user (cookie-based)
    UrlParam,              // ?variant=a
}

enum TestStatus {
    Draft,
    Running,
    Paused,
    Completed,
}
```

### JSON Response Format

The CMS produces responses matching fondu-proxy's expected format:

```json
{
  "selectors": [
    {
      "selector": "#hero-section",
      "op": "replace",
      "components": [
        {
          "_ref": "hero-banner-v2",
          "_variant": "control",
          "_rule": "geo-us-promo",
          "html": "<div class='hero'><h1>Welcome!</h1></div>"
        }
      ]
    },
    {
      "selector": ".sidebar-promos",
      "op": "append",
      "components": [
        {
          "_ref": "sidebar-widget-1",
          "html": "<aside class='promo'>Special Offer</aside>"
        },
        {
          "_ref": "sidebar-widget-2",
          "html": "<aside class='promo'>New Arrivals</aside>"
        }
      ]
    }
  ],
  "_meta": {
    "page": "/homepage",
    "tenant": "acme",
    "generated_at": "2024-01-15T10:30:00Z",
    "cache_ttl": 300,
    "ab_tests": ["homepage-hero-test"],
    "rules_evaluated": ["geo-us-promo", "device-mobile"]
  }
}
```

---

## API Design

### Content Delivery API (Fondu-Facing)

These endpoints are called by fondu-proxy:

#### GET /api/pages/{path}

Returns component directives for a specific page.

**Request:**
```http
GET /api/pages/homepage HTTP/1.1
Host: cms.fondu.example.com
X-Fondu-Tenant: acme
X-Forwarded-For: 203.0.113.50
X-Device-Type: mobile
Cookie: user_segment=premium; ab_user_id=abc123
```

**Response:**
```http
HTTP/1.1 200 OK
Content-Type: application/json
Cache-Control: public, max-age=60, stale-while-revalidate=300
X-Fondu-Cache-Key: acme:homepage:mobile:us:premium
Vary: X-Device-Type, X-Fondu-Tenant

{
  "selectors": [...],
  "_meta": {...}
}
```

**Path Matching:**
- Exact: `/homepage` matches `/homepage`
- Wildcard: `/products/*` matches `/products/widget-a`
- Regex: `/blog/[0-9]+` matches `/blog/123`

#### GET /api/pages/{path}/preview

Preview mode - bypasses caching, shows draft content.

**Request:**
```http
GET /api/pages/homepage/preview HTTP/1.1
X-Fondu-Tenant: acme
X-Preview-Token: eyJhbGciOiJIUzI1NiIs...
```

### Management API

Administrative endpoints for the CMS UI:

#### Components

```http
# List components
GET /api/manage/components
GET /api/manage/components?status=published&search=hero

# Get component
GET /api/manage/components/{id}

# Create component
POST /api/manage/components
{
  "name": "New Hero Banner",
  "html": "<div>...</div>",
  "selector_hint": "#hero",
  "default_op": "replace"
}

# Update component
PUT /api/manage/components/{id}
PATCH /api/manage/components/{id}

# Publish draft
POST /api/manage/components/{id}/publish

# Delete (archive)
DELETE /api/manage/components/{id}

# Version history
GET /api/manage/components/{id}/versions
POST /api/manage/components/{id}/rollback/{version}
```

#### Pages

```http
# List page configs
GET /api/manage/pages

# Get page config
GET /api/manage/pages/{encoded_path}
# e.g., GET /api/manage/pages/homepage
# e.g., GET /api/manage/pages/products%2Fwidgets

# Create/update page config
PUT /api/manage/pages/{encoded_path}
{
  "placements": [
    {
      "selector": "#hero",
      "op": "replace",
      "component_id": "hero-banner-v2"
    }
  ],
  "cache_ttl": 300,
  "enabled": true
}

# Delete page config
DELETE /api/manage/pages/{encoded_path}
```

#### Rules

```http
# CRUD for targeting rules
GET    /api/manage/rules
POST   /api/manage/rules
GET    /api/manage/rules/{id}
PUT    /api/manage/rules/{id}
DELETE /api/manage/rules/{id}

# Test a rule against sample data
POST /api/manage/rules/{id}/evaluate
{
  "context": {
    "country": "US",
    "device_type": "mobile",
    "user_segment": "premium"
  }
}
```

#### A/B Tests

```http
# CRUD for A/B tests
GET    /api/manage/abtests
POST   /api/manage/abtests
GET    /api/manage/abtests/{id}
PUT    /api/manage/abtests/{id}

# Control test lifecycle
POST /api/manage/abtests/{id}/start
POST /api/manage/abtests/{id}/pause
POST /api/manage/abtests/{id}/complete

# Get test results
GET /api/manage/abtests/{id}/results
```

### Authentication & Authorization

Management API requires authentication:

```http
Authorization: Bearer <jwt_token>

# JWT payload
{
  "sub": "user@example.com",
  "tenant": "acme",
  "roles": ["editor", "publisher"],
  "exp": 1705312200
}
```

Role-based permissions:
- **viewer**: Read-only access
- **editor**: Create/edit drafts
- **publisher**: Publish content, manage rules
- **admin**: Full access, manage users

---

## Content Authoring

### Component Editor

The CMS Admin UI provides:

#### Visual HTML Editor

```
┌─────────────────────────────────────────────────────────────────┐
│  Component: Hero Banner v2                          [Save Draft]│
├─────────────────────────────────────────────────────────────────┤
│  ┌─────────────────────────────────────────────────────────────┐│
│  │  [Visual] [HTML] [Preview]                                  ││
│  ├─────────────────────────────────────────────────────────────┤│
│  │                                                              ││
│  │   ┌─────────────────────────────────────────┐               ││
│  │   │  🖼️ Hero Image                          │               ││
│  │   │  ─────────────────────                  │               ││
│  │   │  Welcome to Our Store                   │               ││
│  │   │  [Shop Now Button]                      │               ││
│  │   └─────────────────────────────────────────┘               ││
│  │                                                              ││
│  └─────────────────────────────────────────────────────────────┘│
│                                                                  │
│  Selector: [#hero-section        ]  Operation: [Replace ▼]     │
│                                                                  │
│  Status: Draft                      [Preview] [Publish]         │
└─────────────────────────────────────────────────────────────────┘
```

#### Features

1. **Rich Text Editing**: WYSIWYG with formatting toolbar
2. **HTML Mode**: Direct code editing with syntax highlighting
3. **Live Preview**: See component in context of actual page
4. **Asset Management**: Upload/reference images, icons
5. **Variable Injection**: `{{user.name}}`, `{{product.price}}`
6. **Responsive Preview**: Mobile/tablet/desktop views

### Page Builder

Visual interface for mapping components to pages:

```
┌─────────────────────────────────────────────────────────────────┐
│  Page: /homepage                                                │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  Page Preview                    Placements                      │
│  ┌─────────────────────────┐    ┌─────────────────────────────┐ │
│  │  ┌─────────────────┐    │    │ #hero-section               │ │
│  │  │ #hero-section   │◄───┼────│   Component: Hero Banner v2 │ │
│  │  │                 │    │    │   Op: Replace               │ │
│  │  └─────────────────┘    │    │   [Edit] [Remove]           │ │
│  │                         │    ├─────────────────────────────┤ │
│  │  ┌───────┐ ┌───────┐   │    │ .sidebar                    │ │
│  │  │Content│ │.side- │◄──┼────│   Rule: Premium User Promo  │ │
│  │  │       │ │bar    │   │    │   Fallback: Generic Promo   │ │
│  │  └───────┘ └───────┘   │    │   [Edit] [Remove]           │ │
│  │                         │    ├─────────────────────────────┤ │
│  │  ┌─────────────────┐    │    │ + Add Placement             │ │
│  │  │ #footer-promo   │◄───┼────│                             │ │
│  │  └─────────────────┘    │    └─────────────────────────────┘ │
│  └─────────────────────────┘                                     │
│                                                                  │
│  [Save] [Preview] [Publish]                                     │
└─────────────────────────────────────────────────────────────────┘
```

### Workflow

```
Draft → Review → Scheduled/Published → Archived
  │        │           │
  │        │           └── Live on site
  │        └── Approval queue
  └── Work in progress
```

**Scheduling**: Set future publish/unpublish dates:

```json
{
  "component_id": "black-friday-banner",
  "schedule": {
    "publish_at": "2024-11-29T00:00:00Z",
    "unpublish_at": "2024-12-02T00:00:00Z"
  }
}
```

---

## Caching Strategy

### Multi-Layer Caching

```
┌─────────────────────────────────────────────────────────────────┐
│                        Cache Layers                              │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  Layer 1: Fondu Proxy Cache                                     │
│  ─────────────────────────                                      │
│  • Caches final HTML output                                     │
│  • Key: URL + Vary headers                                      │
│  • TTL: From CMS Cache-Control header                           │
│                                                                  │
│  Layer 2: CMS Response Cache (Fastly Edge)                      │
│  ──────────────────────────────────────────                     │
│  • Caches JSON responses at edge POP                            │
│  • Key: path + tenant + targeting dimensions                    │
│  • TTL: 60s default, configurable per-page                      │
│  • SWR: Serve stale while revalidating                          │
│                                                                  │
│  Layer 3: Storage Cache (In-Compute)                            │
│  ────────────────────────────────────                           │
│  • KV Store has built-in edge caching                           │
│  • Object Store streams directly                                │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### Cache Key Design

Cache keys must account for all factors that affect response:

```
Base Key: {tenant}:{path}

Vary Dimensions:
  + device_type (mobile|tablet|desktop)
  + country (for geo-targeted content)
  + user_segment (if using segmentation)
  + ab_variant (for A/B tests)

Example Keys:
  acme:homepage:mobile:us:premium:hero-test-a
  acme:homepage:desktop:uk:anonymous:hero-test-b
```

### Cache Invalidation

#### Instant Purge (via Surrogate Keys)

```http
# Tag responses with surrogate keys
Surrogate-Key: tenant-acme page-homepage component-hero-v2

# Purge by tag
POST /api/manage/cache/purge
{
  "surrogate_keys": ["component-hero-v2"]
}
```

#### Soft Purge (Preferred)

```http
# Marks content stale, serves stale while fetching fresh
POST /api/manage/cache/soft-purge
{
  "surrogate_keys": ["page-homepage"]
}
```

### TTL Strategy

| Content Type | Default TTL | SWR | SIE | Rationale |
|--------------|-------------|-----|-----|-----------|
| Static components | 1 hour | 4 hours | 24 hours | Rarely changes |
| Promotional content | 5 min | 30 min | 1 hour | Frequent updates |
| Personalized | 1 min | 5 min | 30 min | User-specific |
| A/B test variants | 0 (no-store) | - | - | Must be fresh |
| Preview mode | 0 (no-store) | - | - | Always fresh |

---

## Personalization Engine

### Context Collection

The CMS collects context from the request for targeting:

```rust
struct RequestContext {
    // From Fastly geo database
    geo: GeoContext,

    // From User-Agent parsing
    device: DeviceContext,

    // From request headers/cookies
    user: UserContext,

    // Temporal
    time: TimeContext,
}

struct GeoContext {
    country_code: String,     // "US"
    region: String,           // "California"
    city: String,             // "San Francisco"
    postal_code: String,      // "94102"
    latitude: f64,
    longitude: f64,
    continent: String,        // "NA"
    timezone: String,         // "America/Los_Angeles"
}

struct DeviceContext {
    device_type: DeviceType,  // Mobile, Tablet, Desktop
    browser: String,          // "Chrome"
    browser_version: String,  // "120.0"
    os: String,               // "iOS"
    os_version: String,       // "17.2"
    is_bot: bool,
}

struct UserContext {
    // From cookies/headers
    user_id: Option<String>,
    segments: Vec<String>,    // ["premium", "frequent-buyer"]
    ab_bucket: String,        // Stable hash for A/B tests

    // From JWT (if authenticated)
    is_authenticated: bool,
    user_tier: Option<String>,
}

struct TimeContext {
    timestamp: DateTime,
    day_of_week: u8,          // 0-6
    hour: u8,                 // 0-23
    is_weekend: bool,
    is_business_hours: bool,
}
```

### Rule Evaluation

Rules are evaluated in priority order:

```rust
fn evaluate_rules(
    rules: Vec<Rule>,
    context: &RequestContext
) -> Option<String> {
    // Sort by priority (highest first)
    let sorted_rules = rules.sorted_by(|a, b| b.priority.cmp(&a.priority));

    for rule in sorted_rules {
        if rule.is_active() && rule.all_conditions_match(context) {
            return Some(rule.component_id.clone());
        }
    }

    None // No matching rule
}
```

### A/B Test Assignment

Consistent bucketing using stable user identifier:

```rust
fn get_ab_variant(
    test: &ABTest,
    user_bucket: &str
) -> &Variant {
    // Hash user bucket + test ID for consistency
    let hash = murmurhash3(format!("{}:{}", user_bucket, test.id));
    let bucket = hash % 100;

    // Find variant based on weight distribution
    let mut cumulative = 0;
    for variant in &test.variants {
        cumulative += variant.weight;
        if bucket < cumulative {
            return variant;
        }
    }

    &test.variants[0] // Fallback to first variant
}
```

### Personalization Examples

#### Geo-Based Promotion

```json
{
  "name": "US Holiday Promo",
  "conditions": [
    {"field": "Country", "operator": "Equals", "value": "US"},
    {"field": "DayOfWeek", "operator": "In", "value": "5,6,0"}
  ],
  "component_id": "us-weekend-promo",
  "priority": 100
}
```

#### Device-Specific Content

```json
{
  "name": "Mobile App Promo",
  "conditions": [
    {"field": "DeviceType", "operator": "Equals", "value": "mobile"},
    {"field": "Cookie", "operator": "NotEquals", "value": "app_installed=true"}
  ],
  "component_id": "download-app-banner",
  "priority": 90
}
```

#### User Segment Targeting

```json
{
  "name": "Premium User Exclusive",
  "conditions": [
    {"field": "UserSegment", "operator": "Contains", "value": "premium"}
  ],
  "component_id": "premium-exclusive-offer",
  "priority": 110
}
```

---

## Multi-Tenancy

### Tenant Isolation

Each tenant (customer/site) is completely isolated:

```
┌─────────────────────────────────────────────────────────────────┐
│                      CMS Backend                                 │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  Request: GET /api/pages/homepage                               │
│  Header: X-Fondu-Tenant: acme                                   │
│                                                                  │
│  ┌─────────────────────────────────────────────────────────────┐│
│  │                    Tenant Router                             ││
│  │  1. Extract tenant from header/subdomain                    ││
│  │  2. Validate tenant exists                                  ││
│  │  3. Load tenant config from Config Store                    ││
│  │  4. All subsequent operations scoped to tenant              ││
│  └─────────────────────────────────────────────────────────────┘│
│                           │                                      │
│            ┌──────────────┼──────────────┐                      │
│            ▼              ▼              ▼                      │
│      ┌──────────┐  ┌──────────┐  ┌──────────┐                  │
│      │  Tenant  │  │  Tenant  │  │  Tenant  │                  │
│      │  "acme"  │  │ "globex" │  │ "initech"│                  │
│      └──────────┘  └──────────┘  └──────────┘                  │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### Tenant Configuration

```rust
struct TenantConfig {
    id: String,              // "acme"
    name: String,            // "Acme Corporation"

    // Domains this tenant serves
    domains: Vec<String>,    // ["www.acme.com", "shop.acme.com"]

    // Feature flags
    features: TenantFeatures,

    // Limits
    limits: TenantLimits,

    // Custom settings
    default_cache_ttl: u32,
    timezone: String,
}

struct TenantFeatures {
    ab_testing_enabled: bool,
    personalization_enabled: bool,
    scheduling_enabled: bool,
    api_access_enabled: bool,
}

struct TenantLimits {
    max_components: u32,
    max_pages: u32,
    max_rules: u32,
    max_html_size_kb: u32,
    requests_per_minute: u32,
}
```

### Tenant Identification

Tenants can be identified by:

1. **Header**: `X-Fondu-Tenant: acme` (preferred for API)
2. **Subdomain**: `acme.cms.fondu.com`
3. **Path prefix**: `/tenants/acme/api/...` (legacy support)

---

## Security

### Authentication

#### API Authentication (JWT)

```rust
struct JWTClaims {
    sub: String,           // User ID
    tenant: String,        // Tenant ID
    roles: Vec<String>,    // ["editor", "publisher"]
    exp: u64,              // Expiration timestamp
    iat: u64,              // Issued at
}

// Validation
fn validate_jwt(token: &str, tenant_secret: &str) -> Result<JWTClaims, Error> {
    let decoded = jsonwebtoken::decode::<JWTClaims>(
        token,
        &DecodingKey::from_secret(tenant_secret.as_bytes()),
        &Validation::new(Algorithm::HS256)
    )?;

    // Check expiration
    if decoded.claims.exp < current_timestamp() {
        return Err(Error::TokenExpired);
    }

    Ok(decoded.claims)
}
```

#### Preview Token

Short-lived tokens for content preview:

```rust
struct PreviewToken {
    tenant: String,
    page: String,
    component_version: String,  // "draft" or specific version
    exp: u64,                   // 15 minutes
    created_by: String,
}
```

### Authorization

Role-based access control:

```rust
enum Permission {
    // Components
    ComponentRead,
    ComponentCreate,
    ComponentEdit,
    ComponentPublish,
    ComponentDelete,

    // Pages
    PageRead,
    PageEdit,
    PagePublish,

    // Rules & Tests
    RuleManage,
    ABTestManage,

    // Admin
    CachePurge,
    UserManage,
    TenantConfigure,
}

fn role_permissions(role: &str) -> Vec<Permission> {
    match role {
        "viewer" => vec![ComponentRead, PageRead],
        "editor" => vec![
            ComponentRead, ComponentCreate, ComponentEdit,
            PageRead, PageEdit,
        ],
        "publisher" => vec![
            ComponentRead, ComponentCreate, ComponentEdit, ComponentPublish,
            PageRead, PageEdit, PagePublish,
            RuleManage, ABTestManage, CachePurge,
        ],
        "admin" => Permission::all(),
        _ => vec![],
    }
}
```

### Content Security

#### HTML Sanitization

All user-provided HTML is sanitized:

```rust
fn sanitize_html(html: &str) -> String {
    ammonia::Builder::default()
        .tags(hashset!["div", "span", "p", "h1", "h2", "h3", "a", "img", ...])
        .tag_attributes(hashmap![
            "a" => hashset!["href", "class", "id"],
            "img" => hashset!["src", "alt", "class"],
        ])
        .url_schemes(hashset!["http", "https"])
        .clean(html)
        .to_string()
}
```

#### Script Injection Prevention

- No `<script>` tags allowed in components
- No inline event handlers (`onclick`, `onerror`, etc.)
- External scripts must be from allowlisted domains
- CSP headers recommended on origin

### Rate Limiting

```rust
// Per-tenant rate limits
struct RateLimiter {
    tenant_id: String,

    // Content API (fondu-proxy calls)
    content_rpm: u32,        // 10,000 requests/minute

    // Management API
    manage_rpm: u32,         // 100 requests/minute

    // Write operations
    write_rpm: u32,          // 20 requests/minute
}
```

---

## Implementation Roadmap

### Phase 1: Core CMS (MVP)

**Goal**: Basic component management and delivery

```
Week 1-2: Foundation
├── Project setup (Rust/Compute@Edge)
├── Storage layer abstraction
├── Basic routing
└── Tenant isolation

Week 3-4: Content Management
├── Component CRUD API
├── Page configuration API
├── Basic versioning (draft/published)
└── JSON response generation

Week 5-6: Integration & Testing
├── Integration with fondu-proxy
├── Preview mode
├── Cache headers
└── Error handling
```

**Deliverables**:
- Working CMS backend on Compute@Edge
- Management API for components and pages
- Integration with fondu-proxy
- Basic admin UI (can be separate project)

### Phase 2: Personalization

**Goal**: Targeting rules and A/B testing

```
Week 7-8: Context & Rules
├── Request context extraction
├── Geo detection (Fastly geo DB)
├── Device detection
├── Rule data model & storage
└── Rule evaluation engine

Week 9-10: A/B Testing
├── A/B test data model
├── Consistent bucketing
├── Variant selection
├── Analytics event emission
└── Test lifecycle management
```

**Deliverables**:
- Geo/device/user targeting
- Rule-based component selection
- A/B testing framework
- Analytics integration hooks

### Phase 3: Advanced Features

**Goal**: Enterprise-ready CMS

```
Week 11-12: Workflow & Scheduling
├── Content approval workflow
├── Scheduled publishing
├── Version history & rollback
└── Audit logging

Week 13-14: Performance & Scale
├── Advanced caching strategies
├── Cache invalidation API
├── Multi-region support
├── Rate limiting
└── Monitoring & alerting
```

**Deliverables**:
- Content workflow (draft → review → publish)
- Scheduling system
- Enterprise caching
- Observability

### Phase 4: Ecosystem

**Goal**: Complete platform

```
Week 15-16: Integrations
├── Webhook notifications
├── External CMS connectors (Contentful, Sanity)
├── Analytics platforms (GA, Segment)
└── CDN invalidation APIs

Week 17-18: Developer Experience
├── CLI tool for local development
├── SDK for custom integrations
├── API documentation
└── Template library
```

---

## Appendix: Technical Specifications

### Rust Dependencies

```toml
[dependencies]
fastly = "0.11"              # Fastly Compute SDK
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
regex = "1.10"
ammonia = "4.0"              # HTML sanitization
jsonwebtoken = "9.0"         # JWT handling
chrono = "0.4"               # Date/time
uuid = "1.0"                 # Unique IDs
murmurhash3 = "0.0.5"        # Consistent hashing
```

### Fastly Configuration

```toml
# fastly.toml
manifest_version = 3
name = "fondu-cms"
language = "rust"

[local_server]
  [local_server.kv_stores]
    [local_server.kv_stores.cms_metadata]
      file = "local/kv_metadata.json"
    [local_server.kv_stores.cms_content]
      file = "local/kv_content.json"

[setup]
  [setup.kv_stores]
    [setup.kv_stores.cms_metadata]
    [setup.kv_stores.cms_content]

  [setup.config_stores]
    [setup.config_stores.cms_config]
      items = { tenants = "..." }
```

### Performance Targets

| Metric | Target | Notes |
|--------|--------|-------|
| P50 Latency | < 10ms | For cached responses |
| P99 Latency | < 50ms | Including storage fetch |
| Throughput | 50k req/s | Per edge POP |
| Cache Hit Rate | > 95% | For content delivery |
| Cold Start | < 100ms | WASM instantiation |

---

## Conclusion

This design provides a complete CMS backend architecture for fondu-proxy, running entirely on Fastly's Compute@Edge platform. Key benefits:

1. **Ultra-low latency**: Content served from edge POPs globally
2. **High reliability**: Fail-open design ensures sites always work
3. **Personalization at scale**: Rules evaluated at the edge
4. **Simple integration**: Drop-in backend for existing fondu-proxy deployments
5. **Enterprise-ready**: Multi-tenancy, security, and workflow support

The phased implementation approach allows for iterative delivery while building toward a full-featured content management platform.
