export type CatalogSort =
  | "release_asc"
  | "release_desc"
  | "title_asc"
  | "title_desc"
  | "favorites"
  | "code_asc"
  | "code_desc";

export type CatalogTimeline = "added" | "release" | "updated";

export const catalogFacetGroups = [
  "ages",
  "languages",
  "categories",
  "genres",
  "fileTypes",
  "miscellanies",
  "circles",
] as const;

export type CatalogFacetGroup = (typeof catalogFacetGroups)[number];
export type CatalogFacetState = "off" | "include" | "exclude";

export interface CatalogFacetSelection {
  include: string[];
  exclude: string[];
}

export type CatalogFacetFilters = Record<CatalogFacetGroup, CatalogFacetSelection>;

export interface CatalogFilters {
  search: string;
  category: string;
  tag: string;
  sort: CatalogSort;
}

export interface CatalogRouteState extends CatalogFilters {
  timeline: CatalogTimeline;
  month: string;
  page: number;
}

export interface CatalogBrowseRequest extends CatalogFilters {
  facets: CatalogFacetFilters;
  timeline: CatalogTimeline;
  month: string;
  day: string;
  limit: number;
  offset: number;
}

export interface CatalogNamedRef {
  name: string;
  nameEnglish: string;
}

export interface CatalogCategory {
  code: string;
  name: string;
  nameEnglish: string;
}

export interface CatalogWork {
  code: string;
  sourceCode: string;
  title: string;
  titleEnglish: string;
  addedDate: string;
  releaseDate: string;
  updatedDate: string;
  ageRating: string;
  releaseType: string;
  mainImageUrls: string[];
  thumbnailUrls: string[];
  circles: CatalogNamedRef[];
  categories: CatalogCategory[];
  tags: CatalogNamedRef[];
  synthetic: boolean;
}

export interface CatalogRom {
  name: string;
  size: string;
  crc: string;
  md5: string;
  sha1: string;
  sha256: string;
  fileCount: number | null;
  updateDate: string;
  version: string;
}

export interface CatalogRomEntry {
  entryIndex: number;
  path: string;
  extension: string;
  isDirectory: boolean;
  size: string | null;
  crc32: string;
  md5: string;
  sha1: string;
  sha256: string;
  hashStatus: string;
}

export interface CatalogRomContents {
  status: string;
  archiveFormat: string;
  entryCount: number | null;
  totalUncompressedSize: string | null;
  truncated: boolean;
  entries: CatalogRomEntry[];
}

export interface CatalogRanking {
  range: string;
  rank: number;
}

export interface CatalogRating {
  score: number;
  ratingCount: number | null;
  totalSales: number | null;
  rankings: CatalogRanking[];
}

export type CatalogRelationDirection = "parent" | "child" | "sibling";

export interface CatalogRelatedWork {
  code: string;
  title: string;
  titleEnglish: string;
  relationTypeCode: string;
  relationTypeLabel: string;
  direction: CatalogRelationDirection;
  thumbnailUrls: string[];
}

export interface CatalogDescriptionVersion {
  version: number;
  html: string;
}

export interface CatalogDescriptions {
  included: boolean;
  versions: CatalogDescriptionVersion[];
}

export interface CatalogWorkDetail extends CatalogWork {
  sampleImageUrls: string[];
  fileFormats: CatalogCategory[];
  supportedLanguages: CatalogCategory[];
  miscellanies: CatalogCategory[];
  roms: CatalogRom[];
  relatedWorks: CatalogRelatedWork[];
  rating: CatalogRating | null;
  descriptions: CatalogDescriptions;
}

export type CatalogRecommendationLaneKey = "same_circle" | "similar";

export type CatalogRecommendationReasonKind =
  | "same_circle"
  | "shared_tag"
  | "shared_category"
  | "shared_miscellany"
  | "shared_file_format"
  | "shared_language";

export interface CatalogRecommendationReason {
  kind: CatalogRecommendationReasonKind;
  key: string;
  label: string;
  labelEnglish: string;
}

export interface CatalogRecommendationItem {
  work: CatalogWork;
  score: number;
  reasons: CatalogRecommendationReason[];
}

export interface CatalogRecommendationLane {
  key: CatalogRecommendationLaneKey;
  items: CatalogRecommendationItem[];
}

export interface CatalogRecommendations {
  anchorWorkCode: string;
  lanes: CatalogRecommendationLane[];
}

export interface CatalogFacet {
  key: string;
  label: string;
  labelEnglish: string;
  count: number;
}

export type CatalogFacetCatalog = Record<CatalogFacetGroup, CatalogFacet[]>;

export interface CatalogSnapshot {
  id: string;
  realWorks: number;
  syntheticWorks: number;
}

export interface CatalogMonthBucket {
  month: string;
  count: number;
}

export interface CatalogDayBucket {
  day: string;
  count: number;
}

export interface CatalogContextRequest {
  category: string;
  tag: string;
  facets: CatalogFacetFilters;
  timeline: CatalogTimeline;
}

export interface CatalogContext {
  minMonth: string;
  maxMonth: string;
  defaultMonth: string;
  months: CatalogMonthBucket[];
  facets: CatalogFacetCatalog;
  snapshot: CatalogSnapshot;
}

export interface CatalogBrowsePage {
  items: CatalogWork[];
  total: number;
  unfilteredTotal: number;
  limit: number;
  offset: number;
  hasMore: boolean;
  categories: CatalogFacet[];
  tags: CatalogFacet[];
  facets: CatalogFacetCatalog;
  dayBuckets: CatalogDayBucket[];
  snapshot: CatalogSnapshot;
}

export interface CatalogGateway {
  browse(request: CatalogBrowseRequest): Promise<CatalogBrowsePage>;
  context?(request: CatalogContextRequest): Promise<CatalogContext>;
}

export interface CatalogDetailGateway {
  read(code: string): Promise<CatalogWorkDetail>;
  readWorks(codes: string[]): Promise<CatalogWork[]>;
  readRomContents(workCode: string, romPosition: number): Promise<CatalogRomContents>;
  readRecommendations(code: string): Promise<CatalogRecommendations>;
}
