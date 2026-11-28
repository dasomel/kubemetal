import type { ServiceAccess } from '../types/ipc';

// API-only endpoints have nothing to show in a browser: the S3 API answers a bare GET
// with an S3 XML error, so a dashboard shortcut opens an empty-looking page. They stay
// in the Access console, which shows the URL and credentials an S3 client needs.
const API_ONLY_SERVICES = new Set(['SeaweedFS S3 API']);

export const dashboardEndpoints = (services: ServiceAccess[]): ServiceAccess[] =>
  services.filter((service) => !API_ONLY_SERVICES.has(service.service));
