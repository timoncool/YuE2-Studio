# These pinned ggml copies default to Accelerate's new ILP64 API, added in 13.3.
# Keep Accelerate enabled and use its original LP64 API on earlier deployments.
if(CMAKE_OSX_DEPLOYMENT_TARGET VERSION_LESS "13.3")
    add_compile_options(-UACCELERATE_NEW_LAPACK -UACCELERATE_LAPACK_ILP64)
endif()
