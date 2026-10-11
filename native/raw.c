/* ABI adapter only. Demosaicing remains in the existing LibRaw library. */
#include <libraw/libraw.h>
#include <string.h>
#include <stdio.h>

libraw_processed_image_t *sf_decode_raw(const char *path, int half_size,
    int camera_wb, int *error) {
    libraw_data_t *raw = libraw_init(0);
    if (!raw) { *error = LIBRAW_UNSUFFICIENT_MEMORY; return NULL; }
    /* Match rawpy's order: open/unpack with decoder defaults, then set the
       postprocess parameters. Hasselblad TIFF identification consults WB. */
    *error = libraw_open_file(raw, path);
    if (!*error) *error = libraw_unpack(raw);
    raw->params.output_color = 6; /* ACES, same as rawpy.ColorSpace.ACES */
    raw->params.output_bps = 16;
    raw->params.no_auto_bright = 1;
    raw->params.gamm[0] = 1.0;
    raw->params.gamm[1] = 1.0;
    raw->params.half_size = half_size;
    raw->params.use_camera_wb = camera_wb;
    /* rawpy explicitly enables the camera matrix and disables FBDD. */
    raw->params.use_camera_matrix = 1;
    raw->params.fbdd_noiserd = 0;
    if (!*error) *error = libraw_dcraw_process(raw);
    libraw_processed_image_t *result = NULL;
    if (!*error) result = libraw_dcraw_make_mem_image(raw, error);
    libraw_close(raw);
    return result;
}

unsigned int sf_raw_width(const libraw_processed_image_t *p) { return p->width; }
unsigned int sf_raw_height(const libraw_processed_image_t *p) { return p->height; }
unsigned int sf_raw_colors(const libraw_processed_image_t *p) { return p->colors; }
unsigned int sf_raw_bits(const libraw_processed_image_t *p) { return p->bits; }
unsigned int sf_raw_bytes(const libraw_processed_image_t *p) { return p->data_size; }
const unsigned char *sf_raw_pixels(const libraw_processed_image_t *p) { return p->data; }

/* Metadata ABI only; correction and resampling are implemented in Rust. */
int sf_raw_metadata(const char *path, char *make, char *model, char *lens_make,
                    char *lens, float *focal, float *aperture) {
    libraw_data_t *raw = libraw_init(0);
    if (!raw) return LIBRAW_UNSUFFICIENT_MEMORY;
    int error = libraw_open_file(raw, path);
    if (!error) {
        snprintf(make, 128, "%s", raw->idata.make);
        snprintf(model, 128, "%s", raw->idata.model);
        snprintf(lens_make, 128, "%s", raw->lens.LensMake);
        snprintf(lens, 128, "%s", raw->lens.Lens);
        *focal = raw->other.focal_len;
        *aperture = raw->other.aperture;
    }
    libraw_close(raw);
    return error;
}
