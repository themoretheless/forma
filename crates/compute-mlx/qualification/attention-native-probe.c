#include <mlx/c/fast.h>
#include <mlx/c/ops.h>
#include <mlx/c/device.h>
#include <mlx/c/stream.h>
#include <mlx/c/error.h>
#include <mlx/c/version.h>
#include <stdio.h>
#include <math.h>
#include <stdlib.h>
static void error(const char *msg,void *ctx){fprintf(stderr,"MLX error: %s\n",msg);}
static void run(mlx_stream s,const char *name,int nq,int nk,int hq,int hk,int d,int dv,int mode){
 int qs[]={1,hq,nq,d},ks[]={1,hk,nk,d},vs[]={1,hk,nk,dv},ms[]={nq,nk};
 float *q=calloc(hq*nq*d,sizeof(float)),*k=calloc(hk*nk*d,sizeof(float)),*v=calloc(hk*nk*dv,sizeof(float));
 bool *bm=calloc(nq*nk,sizeof(bool));float *am=calloc(nq*nk,sizeof(float));
 for(int h=0;h<hk;h++)for(int j=0;j<nk;j++)for(int z=0;z<dv;z++)v[(h*nk+j)*dv+z]=10*h+j+1;
 for(int i=0;i<nq;i++)for(int j=0;j<nk;j++){bm[i*nk+j]=mode==3 && i%2==1 && j==0;am[i*nk+j]=(mode==4&&i%2==1&&j==0)?0:-INFINITY;}
 mlx_array qa=mlx_array_new_data(q,qs,4,MLX_FLOAT32),ka=mlx_array_new_data(k,ks,4,MLX_FLOAT32),va=mlx_array_new_data(v,vs,4,MLX_FLOAT32),mask={0};
 if(mode==1||mode==3)mask=mlx_array_new_data(bm,ms,2,MLX_BOOL);
 if(mode==2||mode==4)mask=mlx_array_new_data(am,ms,2,MLX_FLOAT32);
 mlx_array out=mlx_array_new(),dense=mlx_array_new();
 const char *mm=mode==5?"causal":mode?"array":"";
 int code=mlx_fast_scaled_dot_product_attention(&out,qa,ka,va,1,mm,mask,(mlx_array){0},s);
 if(!code)code=mlx_contiguous(&dense,out,false,s);
 if(!code)code=mlx_array_eval(dense);
 printf("%s code=%d Q=[1,%d,%d,%d] K=[1,%d,%d,%d] Vdim=%d row_first:",name,code,hq,nq,d,hk,nk,d,dv);
 if(!code){const float *data=mlx_array_data_float32(dense);for(int h=0;h<hq;h++){printf(" [");for(int i=0;i<nq&&i<5;i++)printf(" %.8g",data[(h*nq+i)*dv]);printf(" ]");}}
 puts("");fflush(stdout);
 mlx_array_free(qa);mlx_array_free(ka);mlx_array_free(va);if(mask.ctx)mlx_array_free(mask);mlx_array_free(out);mlx_array_free(dense);
 free(q);free(k);free(v);free(bm);free(am);
}
int main(){
 mlx_set_error_handler(error,NULL,NULL);
 mlx_device d=mlx_device_new_type(MLX_GPU,0);bool available=false;mlx_device_is_available(&available,d);if(!available)return 2;
 mlx_stream s=mlx_stream_new_device(d);mlx_string ver=mlx_string_new();mlx_version(&ver);printf("MLX %s direct C GPU SDPA probe\n",mlx_string_data(ver));mlx_string_free(ver);
 run(s,"fallback_bool_allfalse",4,3,1,1,3,5,1);
 run(s,"fallback_additive_allinf",4,3,1,1,3,5,2);
 run(s,"fallback_bool_mixed",4,3,1,1,3,5,3);
 run(s,"fallback_additive_mixed",4,3,1,1,3,5,4);
 run(s,"vector_bool_allfalse",4,4,1,1,64,64,1);
 run(s,"vector_additive_allinf",4,4,1,1,64,64,2);
 run(s,"full_bool_mixed",16,8,1,1,64,64,3);
 run(s,"full_additive_mixed",16,8,1,1,64,64,4);
 run(s,"fallback_causal_q_gt_k",4,2,1,1,3,5,5);
 run(s,"fallback_causal_q_lt_k",2,4,1,1,3,5,5);
 run(s,"fallback_GQA_Dv_unequal",3,4,4,2,3,5,0);
 run(s,"vector_GQA_Dv_192_128",2,4,4,2,192,128,0);
 mlx_synchronize(s);mlx_stream_free(s);mlx_device_free(d);return 0;
}
