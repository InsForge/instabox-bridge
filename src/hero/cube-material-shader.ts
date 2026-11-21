export const cubeFragmentSource = `
    precision highp float;
    uniform vec3 metal;
    #ifdef GROUP_LOGOS
    uniform sampler2D logoTexture;
    uniform vec2 logoDimensions;
    uniform float logoOpacity;
    #endif
    uniform float intensity;
    uniform vec3 lightPosition;
    uniform float lightWidth;
    uniform float grain;
    uniform float opening;
    uniform float innerLight;
    uniform float scatterSpread;
    uniform vec3 interiorColor;
    varying float surfacePart;
    varying vec3 localPosition;
    varying vec3 localNormal;
    varying vec3 worldPosition;
    varying vec3 worldNormal;
    void main() {
      vec3 n = normalize(worldNormal);
      vec3 source = lightPosition;
      vec3 toLight = source - worldPosition;
      vec3 l = normalize(toLight);
      #ifdef ORTHOGRAPHIC_CAMERA
      vec3 v = vec3(0.0, 0.0, 1.0);
      #else
      vec3 v = normalize(vec3(0.0, 0.0, 1600.0) - worldPosition);
      #endif
      // Stretch only the horizontal beam radius for a soft elliptical spotlight.
      vec2 beam = vec2((worldPosition.x - source.x) / (toLight.z * lightWidth),
        (worldPosition.y - source.y) / toLight.z - 0.46);
      float spot = exp(-dot(beam, beam) / 0.04);
      float diffuse = max(dot(n, l), 0.0);
      float specular = pow(max(dot(n, normalize(l + v)), 0.0), 24.0);
      float noise = fract(sin(dot(gl_FragCoord.xy, vec2(12.9898, 78.233))) * 43758.5453) - 0.5;
      float illumination = spot * intensity;
      vec3 color = metal * (0.025 + illumination * diffuse * 2.3);
      color += vec3(0.055) * specular * illumination;
      color += noise * grain * 0.1 * illumination * diffuse;
      float revealed = min(1.0, opening / 30.0) * innerLight;
      vec3 interiorPosition = localPosition;
      if (surfacePart > 0.5 && surfacePart < 1.5) interiorPosition.y -= opening;
      vec3 interiorRay = vec3(0.0, 184.0, 0.0) - interiorPosition;
      float incidence = max(0.0, dot(normalize(localNormal), normalize(interiorRay)));
      float incoming = 210000.0 * incidence / max(16000.0, dot(interiorRay, interiorRay));
      if (surfacePart > 2.5) {
        color = metal * 0.025 + interiorColor * incoming * revealed;
      } else if (surfacePart > 1.5) {
        float emission = 2.4 * exp(-dot(localPosition.xz, localPosition.xz) / 6000.0);
        color = metal * 0.025 + interiorColor * emission * revealed;
      } else if (surfacePart > 0.5 && localNormal.y > 0.0) {
        color += interiorColor * incoming * revealed;
      }
      if (surfacePart < 0.5 && abs(localNormal.y) < 0.1 && localPosition.y >= -192.0) {
        float belowGap = max(0.0, localPosition.y + 192.0);
        float acrossFace = abs(localNormal.x) >= abs(localNormal.z) ? localPosition.z : localPosition.x;
        float width = 155.0 + belowGap * 0.3;
        float scatter = 0.5 * exp(-belowGap / (30.0 + scatterSpread * 0.9)) * exp(-pow(abs(acrossFace / width), 4.0));
        color += interiorColor * scatter * revealed;
      }
      #ifdef GROUP_LOGOS
      if (logoDimensions.x > 0.0 && surfacePart > 0.5 && surfacePart < 1.5 && localNormal.y < -0.5) {
        vec2 uv = vec2(localPosition.x, localPosition.z) / logoDimensions + 0.5;
        if (uv.x >= 0.0 && uv.x <= 1.0 && uv.y >= 0.0 && uv.y <= 1.0) {
          vec4 logo = texture2D(logoTexture, uv);
          vec3 litLogo = logo.rgb * clamp(0.6 + illumination * diffuse * 0.2, 0.0, 1.0);
          color = mix(color, litLogo, logo.a * logoOpacity);
        }
      }
      #endif
      gl_FragColor = vec4(max(color, vec3(0.0)), 1.0);
    }
  `;
